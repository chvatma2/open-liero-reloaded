//! How Windows saw a run end (`olr_log::native`): its event log's XML read
//! for the previous run's crash, hang or display driver reset.

use std::path::Path;

use olr_log::native::{self, Run};

/// Windows 11's form: named data.
const CRASH_11: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Application Error' Guid='{a0e9b465-b939-57d7-b27d-95d8e925ff57}'/><EventID>1000</EventID><Level>2</Level><TimeCreated SystemTime='2026-11-04T12:26:01.1844962Z'/><Channel>Application</Channel></System><EventData><Data Name='AppName'>olr.exe</Data><Data Name='AppVersion'>0.0.0.0</Data><Data Name='AppTimeStamp'>00000000</Data><Data Name='ModuleName'>amdvlk64.dll</Data><Data Name='ModuleVersion'>32.0.21042.62</Data><Data Name='ModuleTimeStamp'>65a0b1c2</Data><Data Name='ExceptionCode'>c0000005</Data><Data Name='FaultingOffset'>0000000000123456</Data><Data Name='ProcessId'>0x640</Data><Data Name='ProcessCreationTime'>0x1dd6c88b0b0673e</Data><Data Name='AppPath'>C:\\Users\\ann\\Downloads\\olr\\olr.exe</Data><Data Name='ModulePath'>C:\\WINDOWS\\System32\\DriverStore\\amdvlk64.dll</Data><Data Name='IntegratorReportId'>39d4064e</Data><Data Name='PackageFullName'></Data><Data Name='PackageRelativeAppId'/></EventData></Event>";

/// Windows 10's form: the same data by position, the id without `0x`.
const CRASH_10: &str = "<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Application Error'/><EventID Qualifiers='0'>1000</EventID><TimeCreated SystemTime='2026-11-04T12:26:01.5Z'/></System><EventData><Data>olr.exe</Data><Data>0.0.0.0</Data><Data>00000000</Data><Data>ntdll.dll</Data><Data>10.0.19041.1</Data><Data>a1b2c3d4</Data><Data>c00000fd</Data><Data>00000000000a1b2c</Data><Data>640</Data></EventData></Event>";

const HANG: &str = "<Event><System><Provider Name='Application Hang'/><EventID>1002</EventID><TimeCreated SystemTime='2026-11-04T12:26:10.2Z'/></System><EventData><Data Name='AppName'>olr.exe</Data><Data Name='AppVersion'>0.0.0.0</Data><Data Name='ProcessId'>0x640</Data><Data Name='StartTime'>0x1dd5f24e7d2a5d1</Data><Data Name='TerminationTime'>4294967295</Data><Data Name='HangType'>Quiesce</Data></EventData></Event>";

const RESET: &str = "<Event><System><Provider Name='Display'/><EventID Qualifiers='0'>4101</EventID><TimeCreated SystemTime=\"2026-11-04T12:25:59.9Z\"/></System><EventData><Data>amdkmdag</Data><Data></Data></EventData></Event>";

/// The crashed run: `2026-11-04_12-25-27.225Z_1600.log`, pid 1600 (0x640).
fn run() -> Run {
    native::run_of(Path::new("logs/2026-11-04_12-25-27.225Z_1600.log")).unwrap()
}

#[test]
fn a_run_is_read_from_its_log_name() {
    let r = run();
    assert_eq!((r.pid, r.start.as_str()), (1600, "2026-11-04T12:25:27.225"));
    assert_eq!(native::run_of(Path::new("2026-11-04_12-25-27.225Z_1600-2.log")).unwrap().pid, 1600);
    assert!(native::run_of(Path::new("notes.log")).is_none());
}

#[test]
fn a_crash_names_its_module_and_exception() {
    let lines = native::findings(CRASH_11, "", "olr.exe", &run());
    assert_eq!(
        lines,
        ["Windows recorded the previous run's crash: exception 0xc0000005 (an access violation) \
          in amdvlk64.dll 32.0.21042.62 at offset 0x0000000000123456 \
          (C:\\WINDOWS\\System32\\DriverStore\\amdvlk64.dll)"]
    );
    // Windows 10 by position, and a stack overflow named.
    let lines = native::findings(CRASH_10, "", "OLR.EXE", &run());
    assert_eq!(
        lines,
        ["Windows recorded the previous run's crash: exception 0xc00000fd (a stack overflow) \
          in ntdll.dll 10.0.19041.1 at offset 0x00000000000a1b2c"]
    );
}

#[test]
fn a_hang_and_a_driver_reset_are_said() {
    let app = format!("{HANG}{CRASH_11}");
    let lines = native::findings(&app, RESET, "olr.exe", &run());
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("not responding")), "{lines:?}");
    assert!(
        lines.contains(&"Windows reset the display driver (amdkmdag) during the previous run, at 2026-11-04 12:25:59 UTC: it stopped responding".to_string()),
        "{lines:?}"
    );
}

#[test]
fn another_process_or_an_earlier_event_is_not_the_run() {
    // Another pid, another program, an event before the run started.
    let other = CRASH_11.replace("0x640", "0x641");
    let program = CRASH_11.replace(">olr.exe<", ">other.exe<");
    let before = RESET.replace("12:25:59.9", "12:25:27.1");
    let lines = native::findings(&format!("{other}{program}"), &before, "olr.exe", &run());
    assert_eq!(lines, [native::NOTHING_RECORDED]);
}

#[test]
fn damaged_xml_finds_nothing() {
    for xml in ["", "<Event", "<Event><EventData><Data Name='AppName'>olr.exe", "garbage</Data>"] {
        assert_eq!(native::findings(xml, xml, "olr.exe", &run()), [native::NOTHING_RECORDED], "{xml}");
    }
}

#[test]
fn escaped_text_is_read() {
    let xml = CRASH_11.replace(
        "amdvlk64.dll</Data><Data Name='ModuleVersion'>",
        "a&amp;b.dll</Data><Data Name='ModuleVersion'>",
    );
    let lines = native::findings(&xml, "", "olr.exe", &run());
    assert!(lines[0].contains(" in a&b.dll "), "{lines:?}");
}
