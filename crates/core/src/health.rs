//! Thermal state and battery level, for the lid-closed safety limits.

use std::ffi::c_void;

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};

/// `NSProcessInfo.thermalState`, the public summary of the Mac's thermal condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thermal {
    Nominal,
    Fair,
    /// Apple: "reduce use of system resources", the level at which lid-closed mode stops.
    Serious,
    Critical,
}

pub fn thermal_state() -> Thermal {
    match NSProcessInfo::processInfo().thermalState() {
        NSProcessInfoThermalState::Nominal => Thermal::Nominal,
        NSProcessInfoThermalState::Fair => Thermal::Fair,
        NSProcessInfoThermalState::Serious => Thermal::Serious,
        _ => Thermal::Critical,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// Running on battery rather than AC power.
    pub discharging: bool,
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSCopyPowerSourcesList(blob: *const c_void) -> *const c_void;
    fn IOPSGetPowerSourceDescription(blob: *const c_void, ps: *const c_void) -> *const c_void;
    fn IOPSGetProvidingPowerSourceType(blob: *const c_void) -> CFStringRef;
}

/// The internal battery, or `None` on Macs without one.
pub fn battery() -> Option<Battery> {
    // SAFETY: Copy functions return owned references (create rule); Get functions return
    // references owned by `blob` (get rule), which outlives them here.
    unsafe {
        let blob_ref = IOPSCopyPowerSourcesInfo();
        if blob_ref.is_null() {
            return None;
        }
        let blob = CFType::wrap_under_create_rule(blob_ref.cast());
        let list_ref = IOPSCopyPowerSourcesList(blob_ref);
        if list_ref.is_null() {
            return None;
        }
        let list = CFArray::<CFType>::wrap_under_create_rule(list_ref.cast());
        let providing = IOPSGetProvidingPowerSourceType(blob_ref);
        let discharging = !providing.is_null()
            && CFString::wrap_under_get_rule(providing)
                == CFString::from_static_string("Battery Power");

        for ps in list.iter() {
            let desc = IOPSGetPowerSourceDescription(blob_ref, ps.as_CFTypeRef());
            if desc.is_null() {
                continue;
            }
            let desc = CFDictionary::<CFString, CFType>::wrap_under_get_rule(desc.cast());
            let is_internal = desc
                .find(CFString::from_static_string("Type"))
                .and_then(|t| t.downcast::<CFString>())
                .is_some_and(|t| t == CFString::from_static_string("InternalBattery"));
            if !is_internal {
                continue;
            }
            let number = |key: &'static str| {
                desc.find(CFString::from_static_string(key))
                    .and_then(|n| n.downcast::<CFNumber>())
                    .and_then(|n| n.to_i64())
            };
            let (Some(current), Some(max)) = (number("Current Capacity"), number("Max Capacity"))
            else {
                continue;
            };
            if max <= 0 {
                continue;
            }
            let percent = (current * 100 / max).clamp(0, 100) as u8;
            drop(blob);
            return Some(Battery {
                percent,
                discharging,
            });
        }
        None
    }
}
