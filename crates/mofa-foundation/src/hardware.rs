use std::env::consts;

/// Represents the operating system of the host machine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OsClassification {
    MacOS,
    Windows,
    Linux,
    Other(String),
}

/// Represents the CPU family/architecture of the host machine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CpuFamily {
    AppleSilicon,
    X86_64,
    Arm,
    Other(String),
}

/// The type of GPU acceleration detected on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GpuType {
    /// Apple Metal (available on all macOS 10.14+ devices, both Intel and Apple Silicon).
    Metal,
    /// NVIDIA GPU with CUDA support.
    Cuda,
    /// AMD GPU with ROCm support.
    Rocm,
    /// Intel GPU (detected via `sycl-ls` or similar).
    IntelGpu,
}

/// Holds information about the host environment's hardware capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareCapability {
    pub os: OsClassification,
    pub cpu_family: CpuFamily,
    pub gpu_available: bool,
    /// The type of GPU detected, if any.
    pub gpu_type: Option<GpuType>,
    /// Total system memory in bytes
    pub total_memory_bytes: u64,
    /// Currently available system memory in bytes
    pub available_memory_bytes: u64,
}

/// Detects the host machine's hardware capabilities dynamically.
pub fn detect_hardware() -> HardwareCapability {
    let os = match consts::OS {
        "macos" => OsClassification::MacOS,
        "windows" => OsClassification::Windows,
        "linux" => OsClassification::Linux,
        other => OsClassification::Other(other.to_string()),
    };

    let cpu_family = match consts::ARCH {
        "x86_64" => CpuFamily::X86_64,
        "aarch64" => {
            if os == OsClassification::MacOS {
                CpuFamily::AppleSilicon
            } else {
                CpuFamily::Arm
            }
        }
        "arm" => CpuFamily::Arm,
        other => CpuFamily::Other(other.to_string()),
    };

    let (gpu_available, gpu_type) = detect_gpu(&os);

    // Fetch memory stats via sysinfo
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();

    HardwareCapability {
        os,
        cpu_family,
        gpu_available,
        gpu_type,
        total_memory_bytes: sys.total_memory(),
        available_memory_bytes: sys.available_memory(),
    }
}

/// Detects GPU availability and type based on the host OS.
fn detect_gpu(os: &OsClassification) -> (bool, Option<GpuType>) {
    match os {
        OsClassification::MacOS => {
            // Metal is supported on all Macs running macOS 10.14 (Mojave) and later,
            // including both Intel Macs and Apple Silicon Macs.
            (true, Some(GpuType::Metal))
        }
        OsClassification::Windows | OsClassification::Linux => {
            // Check for NVIDIA GPU (CUDA) by running nvidia-smi and verifying
            // that it reports an actual GPU device, not just that the binary exists.
            if check_nvidia_gpu() {
                return (true, Some(GpuType::Cuda));
            }
            // Check for AMD GPU (ROCm) via rocm-smi.
            if check_amd_gpu() {
                return (true, Some(GpuType::Rocm));
            }
            // Check for Intel GPU via sycl-ls.
            if check_intel_gpu() {
                return (true, Some(GpuType::IntelGpu));
            }
            (false, None)
        }
        _ => (false, None),
    }
}

/// Checks for a usable NVIDIA GPU by running `nvidia-smi` and parsing the output
/// to confirm an actual GPU device is listed (not just that the binary exists).
fn check_nvidia_gpu() -> bool {
    std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

/// Checks for a usable AMD GPU by running `rocm-smi` and verifying it exits successfully
/// with device information in the output.
fn check_amd_gpu() -> bool {
    std::process::Command::new("rocm-smi")
        .arg("--showid")
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

/// Checks for an Intel GPU by running `sycl-ls` and verifying it reports a device.
fn check_intel_gpu() -> bool {
    std::process::Command::new("sycl-ls")
        .output()
        .map(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("Intel")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_hardware() {
        let hardware = detect_hardware();
        println!("Detected Hardware Configuration: {:#?}", hardware);

        // Assert that the fields have been populated with something meaningful.
        // We can't do exact asserts since this runs on different CI/dev environments,
        // but it shouldn't be unknown/other usually (unless we are testing on an exotic arch).

        match hardware.os {
            OsClassification::MacOS | OsClassification::Windows | OsClassification::Linux => (),
            OsClassification::Other(_) => {
                // Warning, unexpected OS but not a failure condition technically
            }
        }

        match hardware.cpu_family {
            CpuFamily::AppleSilicon | CpuFamily::X86_64 | CpuFamily::Arm => (),
            CpuFamily::Other(_) => {
                // Warning, unexpected CPU family
            }
        }

        // GPU availability is environment-dependent, we just ensure it doesn't panic
        let _ = hardware.gpu_available;
    }
}
