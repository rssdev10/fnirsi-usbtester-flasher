//! HID-based firmware flasher for FNIRSI devices
//!
//! This module implements the proprietary flashing protocol used by FNIRSI USB testing devices.
//! It uses the hidapi library for cross-platform HID device communication.
//!
//! # Protocol Overview
//!
//! The FNIRSI FNB-58 bootloader uses a custom HID-based protocol with 64-byte reports:
//!
//! - **Bytes 0-62**: Command/data payload
//! - **Byte 63**: CRC-8-DARC checksum (polynomial 0x39, init 0x00)
//!
//! ## Command Sequence
//!
//! 1. **Device Info** (0x26): Query device information
//! 2. **Erase** (0x28): Erase flash memory
//! 3. **Write Data** (0x2b): Send firmware in 58-byte chunks (6,714 packets for 389,360 bytes)
//! 4. **Finalize** (0x31): Complete flashing and prepare reboot
//! 5. **Status Poll** (0xaa): Query device status (optional, used by Windows app)
//!
//! ## Packet Format
//!
//! Write packets (0x2b) have the following structure:
//! ```text
//! [0]    Command: 0x2b
//! [1]    Length: 0x3a (58 bytes) or shorter for last packet
//! [2-3]  Packet sequence number (little-endian)
//! [4-7]  Flash address (little-endian)
//! [8-65] Firmware data (58 bytes max)
//! [63]   CRC-8-DARC checksum
//! ```
//!
//! # Platform Notes
//!
//! - **macOS**: Requires `sudo` for HID device access. Manual power cycle needed after flash.
//! - **Windows**: Automatic reboot after flash (driver-level USB reset).
//! - **Linux**: Untested, but should work similarly to macOS.

use anyhow::{bail, Context, Result};
use hidapi::{HidApi, HidDevice};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::Read;
use std::path::Path;

// Protocol constants
const REPORT_SIZE: usize = 64; // HID report size in bytes
const PAYLOAD_SIZE: usize = 58; // Firmware data bytes per packet (64 - 6 header bytes)

// HID Command codes
const CMD_DEVICE_INFO: u8 = 0x26; // Query device information
const CMD_ERASE: u8 = 0x28; // Erase flash memory
const CMD_WRITE_DATA: u8 = 0x2b; // Write firmware data
const CMD_FINALIZE: u8 = 0x31; // Finalize and prepare reboot
#[allow(dead_code)]
const CMD_STATUS_POLL: u8 = 0xaa; // Poll device status

/// Calculate CRC-8-DARC checksum for HID packet.
///
/// The FNIRSI bootloader uses CRC-8-DARC algorithm for packet validation:
/// - Polynomial: 0x39
/// - Initial value: 0x00
/// - XOR out: 0x00
/// - Operates on bytes 0-62, result placed in byte 63
///
/// This was reverse-engineered from USB packet captures and verified
/// against all 6,714 write packets in a complete firmware flash.
///
/// # Arguments
///
/// * `data` - 64-byte HID report buffer. Checksum calculated over bytes [0:63]
///
/// # Returns
///
/// Single byte checksum value to be placed at data[63]
fn calculate_checksum(data: &[u8]) -> u8 {
    let mut crc: u8 = 0x00;
    for &byte in &data[..63] {
        crc ^= byte;
        for _ in 0..8 {
            if crc & 0x80 != 0 {
                crc = (crc << 1) ^ 0x39;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// HID-based firmware flasher for FNIRSI devices.
///
/// Manages the HID device connection and implements the complete
/// firmware flashing protocol sequence.
pub struct HidFlasher {
    device: HidDevice,
    verbose: bool,
}

impl HidFlasher {
    /// Open and connect to a FNIRSI device in DFU mode.
    ///
    /// Searches for a connected device with the specified VID/PID (bootloader mode)
    /// and establishes an HID connection.
    ///
    /// # Arguments
    ///
    /// * `vid` - Vendor ID of device in DFU/bootloader mode (from device_registry)
    /// * `pid` - Product ID of device in DFU/bootloader mode (from device_registry)
    /// * `verbose` - Enable verbose debug output
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - HID API initialization fails
    /// - No FNIRSI device found with specified VID/PID
    /// - Device connection fails
    ///
    /// # Platform Requirements
    ///
    /// - **macOS**: Requires `sudo` for HID device access
    /// - **Linux**: May require udev rules or `sudo`
    /// - **Windows**: Should work without elevated privileges
    pub fn open(vid: u16, pid: u16, verbose: bool) -> Result<Self> {
        let api = HidApi::new().context("Failed to initialize HID API")?;

        println!("📡 Looking for FNIRSI device via HID API...");
        println!("   Searching for VID:PID {:04x}:{:04x}", vid, pid);

        // List all HID devices for debugging
        if verbose {
            for device in api.device_list() {
                if device.vendor_id() == vid && device.product_id() == pid {
                    println!(
                        "  Found: VID={:04x} PID={:04x} Interface={}",
                        device.vendor_id(),
                        device.product_id(),
                        device.interface_number()
                    );
                    println!("    Path: {:?}", device.path());
                    println!(
                        "    Usage Page: 0x{:04x}, Usage: 0x{:04x}",
                        device.usage_page(),
                        device.usage()
                    );
                }
            }
        }

        // Try to open the device - hidapi will find the right interface
        let device = api
            .open(vid, pid)
            .context(format!("Failed to open FNIRSI device via HID (VID:PID {:04x}:{:04x})", vid, pid))?;

        println!("✅ HID device opened successfully");

        // Set non-blocking mode
        device
            .set_blocking_mode(true)
            .context("Failed to set blocking mode")?;

        Ok(Self { device, verbose })
    }

    /// Initialize the device and prepare for firmware flashing.
    ///
    /// Sends device info request (0x26) and erase command (0x28) to prepare
    /// the flash memory for new firmware.
    ///
    /// # Protocol Sequence
    ///
    /// 1. Send device info request (0x26)
    /// 2. Wait for device info response (0x27)
    /// 3. Send flash erase command (0x28)
    /// 4. Wait ~10 seconds for erase to complete
    /// 5. Verify erase response (0x29)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Device info request fails
    /// - Erase command fails or times out
    /// - Device returns unexpected response
    pub fn initialize(&self) -> Result<()> {
        println!("🧹 Initializing device...");

        // Send DEVICE_INFO request
        // From pcap: 26 00 45 00 00 00... with checksum 0x36 at byte 63
        let mut request = [0u8; REPORT_SIZE];
        request[0] = CMD_DEVICE_INFO; // 0x26
        request[1] = 0x00;
        request[2] = 0x45;
        // Calculate CRC-8-DARC checksum at byte 63
        request[63] = calculate_checksum(&request);

        println!("  → Sending device info request...");
        if self.verbose {
            println!(
                "    Bytes[0-3]: {:02x?}, checksum[63]: {:02x}",
                &request[..4],
                request[63]
            );
        }

        // Write report (hidapi handles the details)
        self.device
            .write(&request)
            .context("Failed to send device info request")?;

        // Read response
        let mut response = [0u8; REPORT_SIZE];
        let bytes_read = self
            .device
            .read_timeout(&mut response, 5000)
            .context("Failed to read device info response")?;

        if self.verbose {
            println!(
                "    Response ({} bytes): {:02x?}",
                bytes_read,
                &response[..bytes_read.min(16)]
            );
        }

        if bytes_read < 1 || response[0] != 0x27 {
            bail!(
                "Device info failed: got 0x{:02x} (expected 0x27)",
                if bytes_read > 0 { response[0] } else { 0 }
            );
        }
        println!("  ✅ Device info received");

        // Send ERASE command
        // From pcap: 28 06 00 00 00 6f 00 f0 f0 05 with checksum 0xc8
        let mut erase_cmd = [0u8; REPORT_SIZE];
        erase_cmd[0] = CMD_ERASE;
        erase_cmd[1] = 0x06;
        erase_cmd[5] = 0x6f;
        erase_cmd[7] = 0xf0;
        erase_cmd[8] = 0xf0;
        erase_cmd[9] = 0x05;
        // Calculate CRC-8-DARC checksum
        erase_cmd[63] = calculate_checksum(&erase_cmd);

        println!("  → Erasing flash memory...");
        if self.verbose {
            println!(
                "    Bytes[0-10]: {:02x?}, checksum[63]: {:02x}",
                &erase_cmd[..10],
                erase_cmd[63]
            );
        }

        self.device
            .write(&erase_cmd)
            .context("Failed to send erase command")?;

        // Wait for erase response (can take a while)
        println!("  ⏳ Waiting for erase to complete...");
        let mut erase_response = [0u8; REPORT_SIZE];
        let bytes_read = self
            .device
            .read_timeout(&mut erase_response, 30000)
            .context("Failed to read erase response")?;

        if self.verbose {
            println!(
                "    Response ({} bytes): {:02x?}",
                bytes_read,
                &erase_response[..bytes_read.min(16)]
            );
        }

        if bytes_read < 1 || erase_response[0] != 0x29 {
            bail!(
                "Erase failed: got 0x{:02x} (expected 0x29)",
                if bytes_read > 0 { erase_response[0] } else { 0 }
            );
        }
        println!("  ✅ Flash erased");

        Ok(())
    }

    pub fn flash_firmware(self, firmware_data: &[u8]) -> Result<()> {
        let total_chunks = (firmware_data.len() + PAYLOAD_SIZE - 1) / PAYLOAD_SIZE;

        println!(
            "📦 Firmware size: {} bytes ({} chunks)",
            firmware_data.len(),
            total_chunks
        );

        let pb = ProgressBar::new(total_chunks as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                .unwrap()
                .progress_chars("#>-")
        );

        // Track last report for finalize command
        let mut last_report = [0u8; REPORT_SIZE];

        // Write firmware in chunks
        // Each packet contains 58 bytes of firmware data:
        // - bytes [5:62] = 57 bytes at fw[58*(seq-1):58*(seq-1)+57]
        // - byte [62] = 1 byte at fw[58*(seq-1)+57]
        for (chunk_num, chunk) in firmware_data.chunks(PAYLOAD_SIZE).enumerate() {
            pb.set_message(format!("Block {}/{}", chunk_num + 1, total_chunks));

            // Sequence starts at 1
            let sequence = (chunk_num + 1) as u32;

            // Build HID report
            // From pcap analysis:
            // [0] = 0x2b (CMD_WRITE_DATA)
            // [1] = 0x3a (subcommand)
            // [2] = sequence % 50 (wraps at 50: 1,2,...,49,0,1,2,...,49,0,...)
            // [3] = sequence >> 8 (high byte of sequence)
            // [4] = sequence & 0xFF (low byte of sequence)
            // [5:62] = 57 bytes of firmware data
            // [62] = 58th byte of firmware data (or padding if chunk < 58)
            // [63] = CRC-8-DARC checksum
            let mut report = [0u8; REPORT_SIZE];
            report[0] = CMD_WRITE_DATA; // 0x2b
            report[1] = 0x3a; // Subcommand
            report[2] = (sequence % 50) as u8; // Sequence modulo 50
            report[3] = ((sequence >> 8) & 0xFF) as u8; // Seq high byte
            report[4] = (sequence & 0xFF) as u8; // Seq low byte

            // Copy firmware data:
            // First 57 bytes go into positions [5:62]
            // 58th byte goes into position [62]
            if chunk.len() >= 57 {
                report[5..62].copy_from_slice(&chunk[..57]);
                if chunk.len() >= 58 {
                    report[62] = chunk[57];
                }
            } else {
                report[5..5 + chunk.len()].copy_from_slice(chunk);
            }

            // Calculate CRC-8-DARC checksum
            report[63] = calculate_checksum(&report);

            // Debug first few packets
            if self.verbose && chunk_num < 3 {
                println!(
                    "  → Packet {}: {:02x} {:02x} {:02x} {:02x} {:02x} [{}B] cs={:02x}",
                    sequence,
                    report[0],
                    report[1],
                    report[2],
                    report[3],
                    report[4],
                    chunk.len(),
                    report[63]
                );
            }

            // Send write command
            self.device
                .write(&report)
                .context(format!("Failed to send chunk {}", chunk_num))?;

            // Save last report for finalize command
            last_report.copy_from_slice(&report);

            // Read echo response
            let mut response = [0u8; REPORT_SIZE];
            let bytes_read = self
                .device
                .read_timeout(&mut response, 5000)
                .context(format!("Failed to read response for chunk {}", chunk_num))?;

            // Verify response code
            if bytes_read < 1 || response[0] != 0x2c {
                println!(
                    "  ❌ Error at chunk {}: got 0x{:02x} (expected 0x2c)",
                    chunk_num + 1,
                    if bytes_read > 0 { response[0] } else { 0 }
                );
                if self.verbose {
                    println!("      Sent:     {:02x?}", &report[..10]);
                    println!("      Sent cs:  {:02x}", report[63]);
                    println!("      Response: {:02x?}", &response[..bytes_read.min(16)]);
                }
                bail!("Device rejected data at chunk {}", chunk_num + 1);
            }

            pb.inc(1);
        }

        // Send finalize/reboot command (0x31)
        // This is the last write packet with command byte changed to 0x31
        pb.set_message("Finalizing...");

        // Modify the last report: change command from 0x2b to 0x31
        last_report[0] = CMD_FINALIZE;
        // Recalculate checksum with new command byte
        last_report[63] = calculate_checksum(&last_report);

        println!("\n  → Sending finalize command (0x{:02x})...", CMD_FINALIZE);
        self.device
            .write(&last_report)
            .context("Failed to send finalize command")?;

        // Wait for finalize response (0x30)
        let mut response = [0u8; REPORT_SIZE];
        if self.verbose {
            match self.device.read_timeout(&mut response, 2000) {
                Ok(bytes_read) if bytes_read > 0 => {
                    if response[0] == 0x30 {
                        println!("    ✓ Device confirmed finalize (0x30)");
                    } else {
                        println!("    ! Unexpected response: 0x{:02x}", response[0]);
                    }
                }
                Ok(_) => println!("    ! No response from device"),
                Err(e) => println!("    ! Read error: {}", e),
            }
        } else {
            let _ = self.device.read_timeout(&mut response, 2000);
        }

        // Windows app waits ~3 seconds after finalize, then sends status polls
        // This sequence appears to trigger the device reboot
        println!("  → Waiting for device to prepare reboot...");
        std::thread::sleep(std::time::Duration::from_millis(3000));

        // Send status poll sequence to trigger reboot
        println!("  → Sending status polls to trigger reboot...");

        // Poll 1: 0xaa 0x81
        let mut poll1 = [0u8; REPORT_SIZE];
        poll1[0] = CMD_STATUS_POLL;
        poll1[1] = 0x81;
        poll1[63] = calculate_checksum(&poll1);
        let _ = self.device.write(&poll1);
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Try to read response (may fail if device is rebooting)
        let _ = self.device.read_timeout(&mut response, 500);

        // Poll 2: 0xaa 0x82
        let mut poll2 = [0u8; REPORT_SIZE];
        poll2[0] = CMD_STATUS_POLL;
        poll2[1] = 0x82;
        poll2[63] = calculate_checksum(&poll2);
        let _ = self.device.write(&poll2);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = self.device.read_timeout(&mut response, 500);

        // Poll 3: 0xaa 0x82 (repeated)
        let _ = self.device.write(&poll2);

        pb.finish_with_message("✅ Complete!");

        println!();
        println!("✅ Reboot sequence sent, device should restart now...");

        // Wait a bit before closing to allow device to process
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Device handle (self) is consumed and dropped here, closing the HID connection
        Ok(())
    }

    pub fn flash_file<P: AsRef<Path>>(self, path: P) -> Result<()> {
        let path = path.as_ref();

        println!("📂 Reading firmware file: {}", path.display());

        let mut file = File::open(path).context(format!("Failed to open: {}", path.display()))?;

        let mut firmware_data = Vec::new();
        file.read_to_end(&mut firmware_data)
            .context("Failed to read firmware")?;

        if firmware_data.is_empty() {
            bail!("Firmware file is empty");
        }

        self.flash_firmware(&firmware_data)
    }
}
