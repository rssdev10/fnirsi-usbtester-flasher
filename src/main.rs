//! FNIRSI USB Device Flasher
//!
//! A command-line tool for flashing firmware to FNIRSI USB testing devices.
//! Supports the FNB-58 model with automatic device detection and proper HID protocol implementation.
//!
//! # Features
//!
//! - Automatic device detection in bootloader (DFU) mode
//! - Progress bar during firmware flashing
//! - CRC-8-DARC checksum verification for all packets
//! - Cross-platform support (tested on macOS)
//!
//! # Usage
//!
//! ```bash
//! # Flash firmware to connected device
//! fnirsi-usbtester-flasher --firmware path/to/firmware.ufn
//!
//! # List all USB devices
//! fnirsi-usbtester-flasher --list
//!
//! # List supported FNIRSI models
//! fnirsi-usbtester-flasher --list-models
//! ```

mod device_registry;
mod flasher;

use anyhow::{bail, Context, Result};
use clap::Parser;
use console::style;
use dialoguer::Confirm;
use rusb::{Context as UsbContext, Device, DeviceDescriptor, UsbContext as UsbContextTrait};
use std::path::PathBuf;

use device_registry::{DeviceMode, DeviceRegistry};

/// FNIRSI USB Device Flasher
///
/// A tool for flashing firmware to FNIRSI USB testing devices.
/// Supports multiple device models with automatic detection.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Firmware file to flash
    #[arg(short, long, value_name = "FILE")]
    firmware: Option<PathBuf>,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    yes: bool,

    /// List all connected USB devices
    #[arg(short, long)]
    list: bool,

    /// List supported FNIRSI device models
    #[arg(long)]
    list_models: bool,

    /// Enable verbose debug output
    #[arg(short = 'v', long)]
    verbose: bool,
}

fn print_dfu_instructions(_device_name: &str) {
    println!(
        "\n{}",
        style("⚠️  Device is not in DFU/Bootloader mode!")
            .yellow()
            .bold()
    );
    println!(
        "\n{}",
        style("📋 To enter DFU mode for your device:").cyan().bold()
    );
    println!();
    println!("  1. {} from all power sources", style("Disconnect").red());
    println!(
        "  2. {} (center of back/forward toggle)",
        style("Hold down the middle button").green()
    );
    println!(
        "  3. {} while still holding the button",
        style("Connect USB cable").green()
    );
    println!("     → Use Micro-USB cable in the top port labeled \"PC\"");
    println!("  4. Device should now appear in DFU mode");
    println!();
    println!(
        "{}",
        style("After entering DFU mode, run this command again.").cyan()
    );
    println!();
}

fn print_device_info<T: rusb::UsbContext>(
    device: &Device<T>,
    descriptor: &DeviceDescriptor,
    registry: &DeviceRegistry,
) {
    let vid = descriptor.vendor_id();
    let pid = descriptor.product_id();

    if let Some((device_info, mode)) = registry.find_device(vid, pid) {
        let mode_str = match mode {
            DeviceMode::Normal => style(format!("{}", mode)).yellow(),
            DeviceMode::Dfu => style(format!("{}", mode)).green(),
        };

        println!(
            "{} {} (VID:PID {:04x}:{:04x}) - {}",
            style("●").green().bold(),
            style(&device_info.name).cyan().bold(),
            vid,
            pid,
            mode_str
        );
        println!("  └─ Model: {}", device_info.model);
    } else {
        println!(
            "  Bus {:03} Device {:03} - VID:PID {:04x}:{:04x}",
            device.bus_number(),
            device.address(),
            vid,
            pid
        );
    }
}

fn scan_usb_devices(
    registry: &DeviceRegistry,
) -> Result<Vec<(Device<rusb::Context>, DeviceDescriptor, DeviceMode)>> {
    let context = UsbContext::new().context("Failed to initialize USB context")?;
    let devices = context.devices().context("Failed to list USB devices")?;

    let mut fnirsi_devices = Vec::new();

    println!("\n{}", style("🔍 Scanning USB devices...").cyan().bold());
    println!();

    let mut found_fnirsi = false;

    for device in devices.iter() {
        if let Ok(descriptor) = device.device_descriptor() {
            let vid = descriptor.vendor_id();
            let pid = descriptor.product_id();

            if let Some((_device_info, mode)) = registry.find_device(vid, pid) {
                found_fnirsi = true;
                print_device_info(&device, &descriptor, registry);
                fnirsi_devices.push((device, descriptor, mode));
            }
        }
    }

    if !found_fnirsi {
        println!("{}", style("  No FNIRSI devices found").yellow());
    }

    println!();

    Ok(fnirsi_devices)
}

fn flash_firmware(
    _device: Device<rusb::Context>,
    dfu_vid: u16,
    dfu_pid: u16,
    firmware_path: &PathBuf,
    skip_confirmation: bool,
    verbose: bool,
) -> Result<()> {
    if !firmware_path.exists() {
        bail!("Firmware file does not exist: {}", firmware_path.display());
    }

    println!("\n{}", style("📦 Firmware Information").cyan().bold());
    println!("  File: {}", firmware_path.display());
    if let Ok(metadata) = std::fs::metadata(firmware_path) {
        println!("  Size: {} bytes", metadata.len());
    }
    println!();

    if !skip_confirmation {
        let confirmed = Confirm::new()
            .with_prompt("⚠️  This will flash the firmware to your device. Continue?")
            .default(false)
            .interact()?;

        if !confirmed {
            println!("{}", style("❌ Operation cancelled by user").red());
            return Ok(());
        }
        println!();
    }

    println!(
        "{}",
        style("🔥 Starting firmware flash via HID API...")
            .green()
            .bold()
    );
    println!();

    // Use the hidapi-based flasher for better macOS compatibility
    println!("Opening HID device (VID: 0x{:04x}, PID: 0x{:04x})...", dfu_vid, dfu_pid);
    let flasher = flasher::HidFlasher::open(dfu_vid, dfu_pid, verbose)
        .context("Failed to open HID device")?;

    flasher
        .initialize()
        .context("Failed to initialize device")?;

    flasher
        .flash_file(firmware_path)
        .context("Failed to flash firmware")?;

    println!();
    println!(
        "{}",
        style("🎉 Firmware flashed successfully!").green().bold()
    );
    println!();
    println!("{}", style("📋 Next steps:").cyan());
    println!(
        "  1. {} to complete the reboot",
        style("Disconnect and reconnect USB cable").yellow().bold()
    );
    println!("  2. Device will boot into normal operating mode");
    println!();
    println!(
        "{}",
        style("ℹ️  Note: Automatic reboot requires Windows-specific USB drivers").dim()
    );
    println!(
        "{}",
        style("   On macOS/Linux, manual power cycle is needed after flashing").dim()
    );
    println!();

    Ok(())
}

fn list_supported_models(registry: &DeviceRegistry) {
    println!(
        "\n{}",
        style("📱 Supported FNIRSI Device Models").cyan().bold()
    );
    println!();

    for device in registry.list_supported_devices() {
        println!("{} {}", style("●").green(), style(&device.name).bold());
        println!("  Model: {}", device.model);
        println!(
            "  Normal Mode:  VID:PID {:04x}:{:04x}",
            device.vendor_id, device.product_id
        );
        if let (Some(dfu_vid), Some(dfu_pid)) = (device.dfu_vendor_id, device.dfu_product_id) {
            println!("  DFU Mode:     VID:PID {:04x}:{:04x}", dfu_vid, dfu_pid);
        }
        println!();
    }
}

fn list_all_usb_devices(registry: &DeviceRegistry) -> Result<()> {
    let context = UsbContext::new().context("Failed to initialize USB context")?;
    let devices = context.devices().context("Failed to list USB devices")?;

    println!("\n{}", style("🔌 All USB Devices").cyan().bold());
    println!();

    for device in devices.iter() {
        if let Ok(descriptor) = device.device_descriptor() {
            let vid = descriptor.vendor_id();
            let pid = descriptor.product_id();

            // Check if this device can be flashed
            let can_flash = registry.find_device(vid, pid).is_some();
            let flash_marker = if can_flash {
                style(" ⚡").red().bold().to_string()
            } else {
                String::new()
            };

            // Try to get device strings
            let mut manufacturer = String::new();
            let mut product = String::new();
            let mut serial = String::new();

            if let Ok(handle) = device.open() {
                if let Ok(mfg) = handle.read_manufacturer_string_ascii(&descriptor) {
                    manufacturer = mfg;
                }
                if let Ok(prod) = handle.read_product_string_ascii(&descriptor) {
                    product = prod;
                }
                if let Ok(ser) = handle.read_serial_number_string_ascii(&descriptor) {
                    serial = ser;
                }
            }

            println!(
                "{} {} - {}{}",
                style(format!(
                    "Bus {:03} Device {:03}",
                    device.bus_number(),
                    device.address()
                ))
                .dim(),
                style(format!("VID:PID {:04x}:{:04x}", vid, pid))
                    .yellow()
                    .bold(),
                style(format!("Class {:02x}", descriptor.class_code())).cyan(),
                flash_marker
            );

            if !manufacturer.is_empty() {
                println!("  └─ Manufacturer: {}", style(&manufacturer).green());
            }
            if !product.is_empty() {
                println!("  └─ Product:      {}", style(&product).green());
            }
            if !serial.is_empty() {
                println!("  └─ Serial:       {}", style(&serial).dim());
            }

            // Show number of configurations and interfaces
            if let Ok(config) = device.active_config_descriptor() {
                let num_interfaces = config.num_interfaces();
                if num_interfaces > 0 {
                    println!("  └─ Interfaces:   {} interface(s)", num_interfaces);

                    // Show interface details
                    for interface in config.interfaces() {
                        for interface_desc in interface.descriptors() {
                            let class_name = match interface_desc.class_code() {
                                0x01 => "Audio",
                                0x02 => "CDC (Communications)",
                                0x03 => "HID (Human Interface)",
                                0x06 => "Still Imaging",
                                0x07 => "Printer",
                                0x08 => "Mass Storage",
                                0x09 => "Hub",
                                0x0A => "CDC-Data",
                                0x0B => "Smart Card",
                                0x0D => "Content Security",
                                0x0E => "Video",
                                0x0F => "Personal Healthcare",
                                0xDC => "Diagnostic Device",
                                0xE0 => "Wireless Controller",
                                0xEF => "Miscellaneous",
                                0xFE => "Application Specific",
                                0xFF => "Vendor Specific",
                                _ => "Unknown",
                            };

                            println!(
                                "     Interface {} - Class 0x{:02x} ({}) - {} endpoint(s)",
                                interface_desc.interface_number(),
                                interface_desc.class_code(),
                                style(class_name).cyan(),
                                interface_desc.num_endpoints()
                            );
                        }
                    }
                }
            }

            println!();
        }
    }

    println!("{}", style("Legend: ⚡ = Can be flashed with this tool").dim());
    println!();
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = DeviceRegistry::new();

    // Handle list models command
    if cli.list_models {
        list_supported_models(&registry);
        return Ok(());
    }

    // Handle list all USB devices command
    if cli.list {
        list_all_usb_devices(&registry)?;
        return Ok(());
    }

    // Scan for FNIRSI devices
    let fnirsi_devices = scan_usb_devices(&registry)?;

    if fnirsi_devices.is_empty() {
        println!("{}", style("❌ No FNIRSI devices found").red().bold());
        println!();
        println!("💡 Make sure your device is connected via USB");
        println!(
            "💡 Use {} to see supported models",
            style("--list-models").cyan()
        );
        println!("💡 Use --help to see all options.");
        println!("Find the latest updates here: https://github.com/rssdev10/fnirsi-usbtester-flasher");
        return Ok(());
    }

    // If firmware file is specified, proceed with flashing
    if let Some(firmware_path) = cli.firmware {
        // Find a device in DFU mode
        let dfu_device = fnirsi_devices
            .iter()
            .find(|(_, _, mode)| *mode == DeviceMode::Dfu);

        match dfu_device {
            Some((device, descriptor, _)) => {
                let vid = descriptor.vendor_id();
                let pid = descriptor.product_id();
                if let Some((device_info, _)) = registry.find_device(vid, pid) {
                    println!(
                        "{}",
                        style(format!("🎯 Targeting: {}", device_info.name))
                            .green()
                            .bold()
                    );
                    flash_firmware(device.clone(), vid, pid, &firmware_path, cli.yes, cli.verbose)?;
                } else {
                    bail!("Device found but not registered in device_registry");
                }
            }
            None => {
                // No device in DFU mode
                let (_, descriptor, _) = &fnirsi_devices[0];
                let vid = descriptor.vendor_id();
                let pid = descriptor.product_id();

                if let Some((device_info, _)) = registry.find_device(vid, pid) {
                    print_dfu_instructions(&device_info.name);
                }

                bail!("Device is not in DFU mode. Please follow the instructions above.");
            }
        }
    } else {
        // No firmware specified, just show device status
        println!(
            "{}",
            style("💡 To flash firmware, use: --firmware <file>").cyan()
        );
        println!("{}", style("💡 Use --help for more options").cyan());
        println!();
    }

    Ok(())
}
