//! Silent printing of a PDF with SumatraPDF (`-print-to "<printer>" -silent`) and the list of
//! printers on this PC.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Printer {
    pub name: String,
    pub port: String,
    /// The Windows default printer.
    pub default: bool,
    pub offline: bool,
    /// False for PDF, XPS, Fax, OneNote and remote-desktop printers.
    pub physical: bool,
}

/// Names and ports of virtual printers.
pub fn is_virtual(name: &str, port: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let p = port.to_ascii_lowercase();
    ["pdf", "xps", "onenote", "fax", "anydesk", "remote", "teams"].iter().any(|k| n.contains(k))
        || ["portprompt", "nul", "shrfax", "ad_port", "file:", "ts0"].iter().any(|k| p.contains(k))
}

/// Printers from Windows (`Win32_Printer`). Empty off Windows or when PowerShell fails.
pub async fn list_printers() -> Vec<Printer> {
    let script = "Get-CimInstance Win32_Printer | Select-Object Name,PortName,Default,WorkOffline | ConvertTo-Json -Compress";
    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await else { return Vec::new() };
    parse_printers(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_printers(json: &str) -> Vec<Printer> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json.trim()) else { return Vec::new() };
    let rows = match v {
        serde_json::Value::Array(a) => a,
        o @ serde_json::Value::Object(_) => vec![o],
        _ => return Vec::new(),
    };
    rows.iter()
        .filter_map(|r| {
            let name = r["Name"].as_str()?.to_string();
            let port = r["PortName"].as_str().unwrap_or("").to_string();
            Some(Printer { physical: !is_virtual(&name, &port), name, port, default: r["Default"].as_bool().unwrap_or(false), offline: r["WorkOffline"].as_bool().unwrap_or(false) })
        })
        .collect()
}

/// Which printer to use: the configured one (if it exists), else the Windows default when it is a
/// real printer, else the first real printer that is online, else the first real printer.
pub fn choose(printers: &[Printer], configured: &str) -> Option<Printer> {
    let configured = configured.trim();
    if !configured.is_empty() {
        // A configured name is trusted even when the list is empty (the list is a convenience).
        return Some(printers.iter().find(|p| p.name.eq_ignore_ascii_case(configured)).cloned().unwrap_or(Printer { name: configured.to_string(), physical: !is_virtual(configured, ""), ..Default::default() }));
    }
    printers
        .iter()
        .find(|p| p.default && p.physical)
        .or_else(|| printers.iter().find(|p| p.physical && !p.offline))
        .or_else(|| printers.iter().find(|p| p.physical))
        .cloned()
}

/// SumatraPDF from `AOS_SUMATRA`, `D:\aos-ml\bin`, or PATH.
pub fn find_sumatra() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AOS_SUMATRA").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let local = PathBuf::from(r"D:\aos-ml\bin\SumatraPDF.exe");
    if local.is_file() {
        return Some(local);
    }
    std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|d| d.join("SumatraPDF.exe")).find(|p| p.is_file()))
}

/// Send a PDF to a printer, silently. Returns once Sumatra has handed the job to the spooler.
pub async fn print_pdf(pdf: &Path, printer: &str) -> Result<()> {
    let bin = find_sumatra().context("SumatraPDF not found (set AOS_SUMATRA or put it in D:\\aos-ml\\bin)")?;
    anyhow::ensure!(pdf.is_file(), "{} does not exist", pdf.display());
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("-print-to").arg(printer).arg("-silent").arg("-print-settings").arg("fit,paper=A4,monochrome").arg("-exit-when-done").arg(pdf).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let child = cmd.spawn().with_context(|| format!("starting {}", bin.display()))?;
    let out = tokio::time::timeout(Duration::from_secs(120), child.wait_with_output()).await.context("printing took longer than 2 minutes")??;
    if !out.status.success() {
        bail!("SumatraPDF exited with {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{"Name":"Study","PortName":"USB002","Default":false,"WorkOffline":true},{"Name":"Microsoft XPS Document Writer","PortName":"PORTPROMPT:","Default":true,"WorkOffline":false},{"Name":"Home Series","PortName":"USB001","Default":false,"WorkOffline":false},{"Name":"Microsoft Print to PDF","PortName":"PORTPROMPT:","Default":false,"WorkOffline":false}]"#;

    #[test]
    fn picks_a_real_printer_not_the_xps_default() {
        let ps = parse_printers(SAMPLE);
        assert_eq!(ps.len(), 4);
        assert!(!ps[1].physical && !ps[3].physical && ps[0].physical);
        // The Windows default is virtual, so the first online real printer wins.
        assert_eq!(choose(&ps, "").unwrap().name, "Home Series");
        assert_eq!(choose(&ps, "study").unwrap().name, "Study");
        assert_eq!(choose(&ps, "Not Listed").unwrap().name, "Not Listed");
        assert!(choose(&[], "").is_none());
    }

    /// Manual check against a file printer (a "Microsoft Print To PDF" printer whose port is a file
    /// path, so no save dialog appears):
    /// `AOS_TEST_PRINTER="AOS Test PDF" AOS_TEST_PDF=sheet.pdf cargo test -p aos-nora prints_to_a_file_printer -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn prints_to_a_file_printer() {
        let (Ok(printer), Ok(pdf)) = (std::env::var("AOS_TEST_PRINTER"), std::env::var("AOS_TEST_PDF")) else { return };
        print_pdf(Path::new(&pdf), &printer).await.unwrap();
    }

    #[test]
    fn single_printer_json_object_parses() {
        let ps = parse_printers(r#"{"Name":"One","PortName":"USB001","Default":true,"WorkOffline":false}"#);
        assert_eq!(choose(&ps, "").unwrap().name, "One");
    }
}
