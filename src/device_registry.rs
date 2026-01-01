//! Device registry for FNIRSI USB testing devices
//!
//! This module maintains a registry of known FNIRSI devices with their USB VID/PID
//! in both normal operating mode and DFU (Device Firmware Update) bootloader mode.
//!
//! # Supported Devices
//!
//! - **FNB-58**: USB power meter and tester
//!   - Normal mode: VID 0x2e3c, PID 0x5558
//!   - DFU mode: VID 0x0483, PID 0x0038 (STMicroelectronics bootloader)
//!
//! # Device Modes
//!
//! FNIRSI devices have two operating modes:
//!
//! 1. **Normal Mode**: Device functions as a USB tester/meter
//! 2. **DFU Mode**: Bootloader mode for firmware updates
//!
//! To enter DFU mode on FNB-58:
//! 1. Disconnect USB power
//! 2. Hold the middle button (center of back/forward toggle)
//! 3. Connect USB while holding button
//! 4. Release after connection

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// VID in normal operating mode (device functioning as USB tester)
    pub vendor_id: u16,
    /// PID in normal operating mode
    pub product_id: u16,
    /// Device name (e.g., "FNIRSI FNB-58")
    pub name: String,
    /// Short model identifier (e.g., "FNB-58")
    pub model: String,
    /// VID in DFU/bootloader mode (used by flasher.rs)
    pub dfu_vendor_id: Option<u16>,
    /// PID in DFU/bootloader mode (used by flasher.rs)
    pub dfu_product_id: Option<u16>,
}

impl DeviceInfo {
    pub fn is_normal_mode(&self, vid: u16, pid: u16) -> bool {
        self.vendor_id == vid && self.product_id == pid
    }

    pub fn is_dfu_mode(&self, vid: u16, pid: u16) -> bool {
        if let (Some(dfu_vid), Some(dfu_pid)) = (self.dfu_vendor_id, self.dfu_product_id) {
            dfu_vid == vid && dfu_pid == pid
        } else {
            false
        }
    }
}

pub struct DeviceRegistry {
    devices: Vec<DeviceInfo>,
}

impl DeviceRegistry {
    pub fn new() -> Self {
        let mut devices = Vec::new();

        // FNIRSI FNB-58
        devices.push(DeviceInfo {
            vendor_id: 0x2e3c,        // Normal mode: Device functions as USB tester
            product_id: 0x5558,       // Normal mode PID
            name: "FNIRSI FNB-58".to_string(),
            model: "FNB-58".to_string(),
            dfu_vendor_id: Some(0x0483),   // DFU mode: Used by flasher.rs (STM bootloader)
            dfu_product_id: Some(0x0038),  // DFU mode PID: Required for firmware updates
        });

        // Add more FNIRSI devices here as they are discovered
        // Example:
        // devices.push(DeviceInfo {
        //     vendor_id: 0x2e3c,
        //     product_id: 0x5559,
        //     name: "FNIRSI FNB-48".to_string(),
        //     model: "FNB-48".to_string(),
        //     dfu_vendor_id: Some(0x0483),
        //     dfu_product_id: Some(0x0038),
        // });

        Self { devices }
    }

    pub fn find_device(&self, vid: u16, pid: u16) -> Option<(&DeviceInfo, DeviceMode)> {
        for device in &self.devices {
            if device.is_normal_mode(vid, pid) {
                return Some((device, DeviceMode::Normal));
            }
            if device.is_dfu_mode(vid, pid) {
                return Some((device, DeviceMode::Dfu));
            }
        }
        None
    }

    pub fn list_supported_devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    #[allow(dead_code)]
    pub fn add_device(&mut self, device: DeviceInfo) {
        self.devices.push(device);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceMode {
    Normal,
    Dfu,
}

impl std::fmt::Display for DeviceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceMode::Normal => write!(f, "Normal Mode"),
            DeviceMode::Dfu => write!(f, "DFU/Bootloader Mode"),
        }
    }
}
