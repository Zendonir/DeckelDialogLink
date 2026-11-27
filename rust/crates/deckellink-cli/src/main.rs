//! DeckelLink command line.
//!
//! Profiles, checks, preparation, sending and receiving (plain and LSV2, drip-feed),
//! machine parameter files (check, compare, read, write with all locks).
//! A transfer to a real port is only started after a typed confirmation
//! immediately before the transfer; nothing is ever sent automatically.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod params;

use std::io::BufRead;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use deckellink_core::clock::{CancelToken, Clock, SystemClock};
use deckellink_core::transfer::lsv2_transfer::{Lsv2ProgramReceiver, Lsv2ProgramSender};
use deckellink_core::transfer::receiver::{process_received, ProgramReceiver};
use deckellink_core::transfer::sender::ProgramSender;
use deckellink_core::transfer::{Event, EventSink, SendState};
use deckellink_core::transport::lsv2_sim::{Lsv2SimConfig, Lsv2SimulatedCnc};
use deckellink_core::transport::rfc2217::Rfc2217Transport;
use deckellink_core::transport::serial::{available_ports, SerialTransport};
use deckellink_core::transport::simulated::{SimulatedTransport, SimulationConfig};
use deckellink_core::transport::tcp::TcpTransport;
use deckellink_core::transport::Transport;

use deckellink_core::model::Profile;
use deckellink_core::nc::analyzer::analyze;
use deckellink_core::nc::transformer::transform;
use deckellink_core::nc::AnalysisResult;
use deckellink_core::profile::{builtin_profiles, parse_profile_text, safety_warnings};
use deckellink_core::storage::{write_new_file, StorageError};
use serde_json::json;

const USAGE: &str = "\
DeckelLink CLI (Rust, Etappen 1-3)

Verwendung:
  deckellink-cli profiles                         Mitgelieferte Profile anzeigen
  deckellink-cli check <datei> [--profile <id|datei.json>]
                                                   NC-Datei prüfen (nur lesen)
  deckellink-cli prepare <datei> <neue-datei> [--profile <id|datei.json>]
                                                   Aufbereitete Transferdaten als NEUE Datei schreiben
  deckellink-cli ports                            Serielle Anschlüsse auflisten
  deckellink-cli send <datei> (--port COM3 | --tcp HOST:PORT | --rfc2217 HOST:PORT | --sim) [--profile ...] [--drip]
                                                   NC-Datei senden; LSV2-Profile senden mit Protokoll,
                                                   --drip = Nachladebetrieb (Start nur an der CNC).
                                                   Reale Ports nur nach getippter Bestätigung.
  deckellink-cli receive <neue-datei> (--port COM3 | --tcp HOST:PORT | --rfc2217 HOST:PORT | --sim) [--profile ...]
                                                   Programm von der CNC empfangen (Ausgabe an der CNC starten)
  deckellink-cli param-check <datei> [--profile ...]
                                                   Maschinenparameterdatei prüfen (Gruppen, Prüfsummenzeilen)
  deckellink-cli param-diff <alt> <neu>            Parameterdateien vergleichen
  deckellink-cli param-read (--port ... | --sim) [--profile ...] [--archiv DIR]
                                                   Parameterausgabe der CNC sichern (DIR/parameters, nie überschrieben)
  deckellink-cli param-write <datei> (--port ... | --sim) --freischalten [--profile ...] [--archiv DIR]
                                                   Parameter schreiben: nur mit Profilfreigabe, Freischaltung,
                                                   frischer Sicherung, Vergleich, getippter Bestätigung
                                                   und anschließender Kontrolle durch erneutes Auslesen
  deckellink-cli analyze-json <profil.json> <datei>
                                                   Maschinenlesbare Analyse (Gleichwertigkeitstests)

Hinweis: Das Programm startet keine Maschinenfunktionen. Abbruch mit Strg+C.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("Fehler: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some(command) = args.first() else {
        println!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    };
    let rest = args.get(1..).unwrap_or_default();
    match command.as_str() {
        "profiles" => {
            let (profiles, problems) = builtin_profiles();
            for p in &profiles {
                println!(
                    "{:<24} {}  [{}]  {}",
                    p.id,
                    p.serial.summary(),
                    p.protocol.mode.as_str(),
                    p.verification_status.label()
                );
            }
            for problem in problems {
                println!("FEHLER: {problem}");
            }
            Ok(ExitCode::SUCCESS)
        }
        "check" => {
            let (file, profile) = file_and_profile(rest)?;
            let data = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let result = analyze(&data, &profile.transfer, Some(&profile.serial));
            print_analysis(&file, &profile, &result);
            Ok(if result.has_errors() { ExitCode::from(1) } else { ExitCode::SUCCESS })
        }
        "prepare" => {
            let (positional, profile) = split_profile_arg(rest)?;
            let [source, target] = positional.as_slice() else {
                return Err(USAGE.into());
            };
            let source = PathBuf::from(source);
            let target = PathBuf::from(target);
            let data = std::fs::read(&source).map_err(|e| format!("{}: {e}", source.display()))?;
            let analysis = analyze(&data, &profile.transfer, Some(&profile.serial));
            if analysis.has_errors() {
                print_analysis(&source, &profile, &analysis);
                return Ok(ExitCode::from(1));
            }
            let prepared = transform(&data, &profile.transfer, false).map_err(|e| e.to_string())?;
            write_new_file(&target, &prepared.data).map_err(|e| match e {
                StorageError::Exists(name) => {
                    format!("{name} existiert bereits - nichts überschrieben")
                }
                other => other.to_string(),
            })?;
            println!("{} Zeilen, {} Bytes -> {}", prepared.line_count, prepared.data.len(), target.display());
            for note in &prepared.applied {
                println!("  - {note}");
            }
            Ok(ExitCode::SUCCESS)
        }
        "ports" => {
            let ports = available_ports();
            if ports.is_empty() {
                println!("Keine seriellen Anschlüsse gefunden.");
            }
            for port in ports {
                println!("{}", port.label());
            }
            Ok(ExitCode::SUCCESS)
        }
        "send" => send(rest),
        "receive" => receive(rest),
        "lsv2-trace" => lsv2_trace(rest),
        "param-check" => params::param_check(rest),
        "param-diff" => params::param_diff(rest),
        "param-read" => params::param_read(rest),
        "param-write" => params::param_write(rest),
        "param-json" => params::param_json(rest),
        "analyze-json" => {
            let [profile_file, file] = rest else {
                return Err(USAGE.into());
            };
            let text = std::fs::read_to_string(profile_file).map_err(|e| e.to_string())?;
            let (profile, _) = parse_profile_text(&text, profile_file, false).map_err(|e| e.to_string())?;
            let data = std::fs::read(file).map_err(|e| e.to_string())?;
            let analysis = analyze(&data, &profile.transfer, Some(&profile.serial));
            let prepared = transform(&data, &profile.transfer, true);
            let findings: Vec<_> = analysis.findings.iter().map(finding_value).collect();
            let transform_json = match prepared {
                Ok(t) => json!({"ok": true, "data": hex(&t.data), "line_count": t.line_count,
                                "offsets": t.line_end_offsets, "applied": t.applied,
                                "appended": t.appended_program_end}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            };
            let out = json!({
                "findings": findings,
                "byte_count": analysis.byte_count,
                "line_count": analysis.line_count,
                "longest_line": analysis.longest_line,
                "has_program_end": analysis.has_program_end,
                "sha256": analysis.sha256,
                "endings": {"crlf": analysis.line_endings.crlf, "lf": analysis.line_endings.lf, "cr": analysis.line_endings.cr},
                "transform": transform_json,
            });
            println!("{out}");
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("Unbekannter Befehl '{other}'\n\n{USAGE}")),
    }
}

fn finding_value(f: &deckellink_core::nc::Finding) -> serde_json::Value {
    json!({"code": f.code, "severity": f.severity.as_str(), "message": f.message, "lines": f.lines, "count": f.count})
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn split_profile_arg(args: &[String]) -> Result<(Vec<String>, Profile), String> {
    let mut positional = Vec::new();
    let mut profile_arg: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--profile" {
            profile_arg = Some(iter.next().ok_or("--profile ohne Wert")?.clone());
        } else {
            positional.push(arg.clone());
        }
    }
    Ok((positional, load_profile(profile_arg.as_deref())?))
}

fn file_and_profile(args: &[String]) -> Result<(PathBuf, Profile), String> {
    let (positional, profile) = split_profile_arg(args)?;
    let [file] = positional.as_slice() else {
        return Err(USAGE.into());
    };
    Ok((PathBuf::from(file), profile))
}

fn load_profile(arg: Option<&str>) -> Result<Profile, String> {
    let (profiles, _) = builtin_profiles();
    let wanted = arg.unwrap_or("dialog11_7n1_rtscts");
    if let Some(profile) = profiles.iter().find(|p| p.id == wanted) {
        return Ok(profile.clone());
    }
    let text = std::fs::read_to_string(wanted).map_err(|e| format!("Profil '{wanted}' nicht gefunden: {e}"))?;
    let (profile, warnings) = parse_profile_text(&text, wanted, false).map_err(|e| e.to_string())?;
    for warning in warnings {
        eprintln!("Hinweis: {warning}");
    }
    Ok(profile)
}

fn print_analysis(file: &std::path::Path, profile: &Profile, result: &AnalysisResult) {
    println!("Datei:   {}", file.display());
    println!("Profil:  {} ({})", profile.display_name, profile.verification_status.label());
    println!(
        "Größe:   {} Bytes, {} Zeilen, längste Zeile {} Zeichen, SHA-256 {}",
        result.byte_count,
        result.line_count,
        result.longest_line,
        result.sha256.get(..16).unwrap_or_default()
    );
    println!("Programmende: {}", if result.has_program_end { "gefunden" } else { "NICHT gefunden" });
    for warning in safety_warnings(profile) {
        println!("Profilhinweis: {warning}");
    }
    if result.findings.is_empty() {
        println!("Keine Auffälligkeiten.");
    }
    for f in &result.findings {
        let lines = f.lines_text();
        println!(
            "[{}] {}{}",
            f.severity.label(),
            f.message,
            if lines.is_empty() { String::new() } else { format!(" (Zeilen {lines})") }
        );
    }
}

/// Parsed connection options.
#[derive(Debug, Default)]
struct Options {
    positional: Vec<String>,
    profile: Option<String>,
    port: Option<String>,
    tcp: Option<String>,
    rfc2217: Option<String>,
    sim: bool,
    archive: Option<String>,
    unlock: bool,
    drip: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--profile" => options.profile = Some(iter.next().ok_or("--profile ohne Wert")?.clone()),
            "--port" => options.port = Some(iter.next().ok_or("--port ohne Wert")?.clone()),
            "--tcp" => options.tcp = Some(iter.next().ok_or("--tcp ohne Wert")?.clone()),
            "--rfc2217" => options.rfc2217 = Some(iter.next().ok_or("--rfc2217 ohne Wert")?.clone()),
            "--sim" => options.sim = true,
            "--archiv" => options.archive = Some(iter.next().ok_or("--archiv ohne Wert")?.clone()),
            "--freischalten" => options.unlock = true,
            "--drip" => options.drip = true,
            _ => options.positional.push(arg.clone()),
        }
    }
    let targets = usize::from(options.port.is_some())
        + usize::from(options.tcp.is_some())
        + usize::from(options.rfc2217.is_some())
        + usize::from(options.sim);
    if targets != 1 {
        return Err("Genau eine Verbindung angeben: --port, --tcp, --rfc2217 oder --sim".into());
    }
    Ok(options)
}

fn make_transport(
    options: &Options,
    profile: &Profile,
    machine_program: Vec<u8>,
) -> Result<Box<dyn Transport>, String> {
    if options.sim {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
        if profile.protocol.is_lsv2() {
            let config = Lsv2SimConfig {
                program: machine_program,
                speed_factor: 50.0,
                buffer_telegrams: if options.drip { 3 } else { 0 },
                execution_s_per_telegram: if options.drip { 0.2 } else { 0.0 },
                ..Lsv2SimConfig::default()
            };
            return Ok(Box::new(Lsv2SimulatedCnc::new(
                profile.serial.clone(),
                profile.protocol.clone(),
                config,
                clock,
            )));
        }
        let config = SimulationConfig { machine_program, speed_factor: 50.0, ..SimulationConfig::default() };
        return Ok(Box::new(SimulatedTransport::new(profile.serial.clone(), config, clock)));
    }
    if let Some(port) = &options.port {
        return Ok(Box::new(SerialTransport::new(port.clone(), profile.serial.clone())));
    }
    if let Some(endpoint) = &options.rfc2217 {
        let (host, port) = split_endpoint(endpoint, "--rfc2217")?;
        return Ok(Box::new(Rfc2217Transport::new(host, port, profile.serial.clone())));
    }
    let endpoint = options.tcp.clone().unwrap_or_default();
    let (host, port) = split_endpoint(&endpoint, "--tcp")?;
    Ok(Box::new(TcpTransport::new(host, port, profile.serial.clone())))
}

/// Splits `HOST:PORT` into host and TCP port.
fn split_endpoint<'a>(endpoint: &'a str, option: &str) -> Result<(&'a str, u32), String> {
    let (host, port) = endpoint.rsplit_once(':').ok_or_else(|| format!("{option} erwartet HOST:PORT"))?;
    let port: u32 = port.parse().map_err(|_| format!("Ungültiger TCP-Port: {port}"))?;
    if host.is_empty() || port == 0 || port > 65535 {
        return Err(format!("{option} erwartet HOST:PORT (Port 1-65535)"));
    }
    Ok((host, port))
}

/// Prints state changes and progress in 10 % steps.
#[derive(Debug, Default)]
struct Console {
    last_decile: Option<u64>,
}

impl EventSink for Console {
    fn emit(&mut self, event: Event) {
        match event {
            Event::SendState(state) => eprintln!("  {}", state.label()),
            Event::ReceiveState(state) => eprintln!("  {}", state.label()),
            Event::Progress(p) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let decile = (p.percent() / 10.0) as u64;
                if self.last_decile != Some(decile) {
                    self.last_decile = Some(decile);
                    eprintln!("  {:5.1} %  Zeile {}/{}", p.percent(), p.current_line, p.total_lines);
                }
            }
            Event::ReceiveProgress(p) => {
                let decile = (p.bytes_received / 1024) as u64;
                if self.last_decile != Some(decile) {
                    self.last_decile = Some(decile);
                    eprintln!("  {} Bytes empfangen", p.bytes_received);
                }
            }
            Event::Traffic(_) | Event::Modem(_) => {}
        }
    }
}

fn confirm(question: &str) -> bool {
    eprintln!("{question}");
    eprint!("Zum Starten SENDEN eintippen (alles andere bricht ab): ");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "SENDEN"
}

fn send(args: &[String]) -> Result<ExitCode, String> {
    let options = parse_options(args)?;
    let profile = load_profile(options.profile.as_deref())?;
    let [file] = options.positional.as_slice() else { return Err(USAGE.into()) };
    let path = PathBuf::from(file);
    let data = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let analysis = analyze(&data, &profile.transfer, Some(&profile.serial));
    print_analysis(&path, &profile, &analysis);
    if analysis.has_errors() {
        eprintln!("Senden gesperrt: Die Datei hat Prüffehler.");
        return Ok(ExitCode::from(1));
    }
    if options.drip && !profile.protocol.is_lsv2() {
        return Err("Nachladebetrieb erfordert ein Profil mit LSV2-Protokoll (z. B. dialog11_lsv2_7e1).".into());
    }
    let payload = transform(&data, &profile.transfer, false).map_err(|e| e.to_string())?;
    let mut transport = make_transport(&options, &profile, Vec::new())?;
    if options.drip {
        eprintln!(
            "\nNACHLADEBETRIEB (LSV2): Die CNC fordert die Sätze während der Bearbeitung an. Das Programm wird \
             ausschließlich an der CNC angewählt und gestartet. Ein Abbruch hier hält die Maschine NICHT an - \
             im Notfall an der Steuerung stoppen."
        );
    }
    if !options.sim {
        let question = format!(
            "\nREALE ÜBERTRAGUNG an {} ({}), Profil {} ({})\n{} Bytes, {} Zeilen. Einlesen an der CNC muss gestartet sein.",
            transport.name(),
            profile.serial.summary(),
            profile.display_name,
            profile.verification_status.label(),
            payload.data.len(),
            payload.line_count
        );
        if !confirm(&question) {
            eprintln!("Nicht gesendet.");
            return Ok(ExitCode::from(1));
        }
    } else {
        eprintln!("\nSIMULATION - keine Verbindung zur Maschine");
    }
    let clock = SystemClock::default();
    let token = CancelToken::new();
    let mut console = Console::default();
    let result = if profile.protocol.is_lsv2() {
        let mut sender =
            Lsv2ProgramSender::new(transport.as_mut(), &profile, &clock, &token, &mut console, options.drip);
        sender.program_name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        sender.send(&payload)
    } else {
        ProgramSender::new(transport.as_mut(), &profile, &clock, &token, &mut console).send(&payload)
    };
    match (&result.state, &result.error) {
        (SendState::Completed, _) => {
            println!("Übertragung abgeschlossen: {} Bytes in {:.1} s", result.bytes_sent, result.duration_s);
            Ok(ExitCode::SUCCESS)
        }
        (_, Some(error)) => {
            eprintln!("{}", error.as_text());
            Ok(ExitCode::from(1))
        }
        (state, None) => {
            eprintln!("{}", state.label());
            Ok(ExitCode::from(1))
        }
    }
}

fn receive(args: &[String]) -> Result<ExitCode, String> {
    let options = parse_options(args)?;
    let profile = load_profile(options.profile.as_deref())?;
    let [file] = options.positional.as_slice() else { return Err(USAGE.into()) };
    let target = PathBuf::from(file);
    if target.exists() {
        return Err(format!("{} existiert bereits - bitte einen neuen Dateinamen angeben", target.display()));
    }
    let demo = b"(SIMULATION)\r\nN10 G00 X0 Y0\r\nN20 M30\r\n".to_vec();
    let mut transport = make_transport(&options, &profile, demo)?;
    eprintln!("Empfangsbereit auf {} - jetzt die Ausgabe an der CNC starten.", transport.name());
    let clock = SystemClock::default();
    let token = CancelToken::new();
    let mut console = Console::default();
    let result = if profile.protocol.is_lsv2() {
        Lsv2ProgramReceiver::new(transport.as_mut(), &profile, &clock, &token, &mut console).receive()
    } else {
        ProgramReceiver::new(transport.as_mut(), &profile, &clock, &token, &mut console).receive()
    };
    if result.raw.is_empty() {
        eprintln!("{} - {}", result.state.label(), result.end_reason.label());
        if let Some(error) = &result.error {
            eprintln!("{}", error.as_text());
        }
        return Ok(ExitCode::from(1));
    }
    let program = process_received(&result, &profile);
    write_new_file(&target, &program.normalized).map_err(|e| e.to_string())?;
    let raw_target = target.with_extension("raw");
    if profile.receive.save_raw && !raw_target.exists() {
        write_new_file(&raw_target, &program.raw).map_err(|e| e.to_string())?;
    }
    println!(
        "{} ({}) - {} Bytes -> {}",
        result.state.label(),
        result.end_reason.label(),
        program.normalized.len(),
        target.display()
    );
    for note in &program.notes {
        println!("  - {note}");
    }
    Ok(if program.complete { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn json_list(config: &serde_json::Value, key: &str) -> Vec<usize> {
    config
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|items| items.iter().filter_map(|v| v.as_u64()).filter_map(|v| usize::try_from(v).ok()).collect())
        .unwrap_or_default()
}

fn json_opt(config: &serde_json::Value, key: &str) -> Option<usize> {
    config.get(key).and_then(serde_json::Value::as_u64).and_then(|v| usize::try_from(v).ok())
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .filter_map(|i| text.get(2 * i..2 * i + 2))
        .filter_map(|h| u8::from_str_radix(h, 16).ok())
        .collect()
}

/// Wire-level trace against the simulated LSV2 CNC with virtual time (parity tests with Python).
fn lsv2_trace(args: &[String]) -> Result<ExitCode, String> {
    use deckellink_core::clock::FakeClock;
    use deckellink_core::transfer::{Direction, ReceiveState};

    let [profile_file, file, mode, config_json] = args else { return Err(USAGE.into()) };
    let text = std::fs::read_to_string(profile_file).map_err(|e| e.to_string())?;
    let (profile, _) = parse_profile_text(&text, profile_file, false).map_err(|e| e.to_string())?;
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(config_json).map_err(|e| e.to_string())?;
    let sim = Lsv2SimConfig {
        never_ready: config.get("never_ready").and_then(serde_json::Value::as_bool).unwrap_or(false),
        buffer_telegrams: json_opt(&config, "buffer_telegrams").unwrap_or(0),
        execution_s_per_telegram: config
            .get("execution_s_per_telegram")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0),
        nak_on_telegrams: json_list(&config, "nak_on_telegrams"),
        drop_ack_on_telegrams: json_list(&config, "drop_ack_on_telegrams"),
        wack_on_telegrams: json_list(&config, "wack_on_telegrams"),
        eot_after_telegrams: json_opt(&config, "eot_after_telegrams"),
        disconnect_after_telegrams: json_opt(&config, "disconnect_after_telegrams"),
        corrupt_bcc_on_sent_telegrams: json_list(&config, "corrupt_bcc_on_sent_telegrams"),
        send_lines_per_telegram: json_opt(&config, "send_lines_per_telegram").unwrap_or(1),
        program: config.get("program").and_then(serde_json::Value::as_str).map(unhex).unwrap_or_default(),
        ..Lsv2SimConfig::default()
    };
    let clock = FakeClock::default();
    let mut cnc = Lsv2SimulatedCnc::new(profile.serial.clone(), profile.protocol.clone(), sim, Arc::new(clock.clone()));
    let token = CancelToken::new();
    let mut events: Vec<Event> = Vec::new();
    let (state, extra, duration) = if mode == "receive" {
        let result = Lsv2ProgramReceiver::new(&mut cnc, &profile, &clock, &token, &mut events).receive();
        let state = match result.state {
            ReceiveState::Completed => "completed",
            ReceiveState::Cancelled => "cancelled",
            ReceiveState::TimedOut => "timed_out",
            _ => "failed",
        };
        (state, hex(&result.raw), result.duration_s)
    } else {
        let payload = transform(&data, &profile.transfer, false).map_err(|e| e.to_string())?;
        let result =
            Lsv2ProgramSender::new(&mut cnc, &profile, &clock, &token, &mut events, mode == "drip").send(&payload);
        let state = match result.state {
            SendState::Completed => "completed",
            SendState::Cancelled => "cancelled",
            _ => "failed",
        };
        (state, hex(&cnc.record().received_program()), result.duration_s)
    };
    let stream = |direction: Direction| -> String {
        let bytes: Vec<u8> = events
            .iter()
            .filter_map(|e| {
                if let Event::Traffic(t) = e {
                    (t.direction == direction).then(|| t.data.clone())
                } else {
                    None
                }
            })
            .flatten()
            .collect();
        hex(&bytes)
    };
    let out = json!({
        "state": state,
        "tx": stream(Direction::Tx),
        "rx": stream(Direction::Rx),
        "data": extra,
        "duration": (duration * 1000.0).round() / 1000.0,
    });
    println!("{out}");
    Ok(ExitCode::SUCCESS)
}
