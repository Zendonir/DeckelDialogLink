//! Machine parameter commands (stage 3): check, compare, read into the archive, write with all locks.
//!
//! Writing requires, in this order: profile `parameters.allow_write`, `--freischalten` plus a
//! confirmation for this session (one CLI run), a backup read in the same run, a clean check,
//! the diff, the write gate, the typed phrase `PARAMETER SCHREIBEN` and finally a
//! verification read. No remote parameter commands are ever sent.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use deckellink_core::clock::{CancelToken, Clock, SystemClock};
use deckellink_core::model::Profile;
use deckellink_core::parameters::archive::{ParameterArchive, StoredParameterSet};
use deckellink_core::parameters::fileformat::{compatibility_problems, parse_parameter_file, ParameterFileInfo};
use deckellink_core::parameters::transfer::{read_parameters, write_parameters};
use deckellink_core::parameters::{
    analyze_parameters, diff_parameters, evaluate_parameter_write_gate, parameter_profile, prepare_parameter_payload,
    sample_parameters, GateInput, ParameterBackupInfo, ParameterCandidate, PARAMETER_WRITE_NOTICE,
    WRITE_CONFIRMATION_PHRASE,
};
use deckellink_core::profile::parse_profile_text;
use deckellink_core::transfer::receiver::{process_received, ReceivedProgram};
use deckellink_core::transfer::SendState;
use serde_json::{json, Value};

use crate::{load_profile, make_transport, parse_options, print_analysis, Console, Options, USAGE};

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn print_structure(info: Option<&ParameterFileInfo>) {
    match info {
        Some(info) => {
            println!("Struktur: {}", info.summary());
            for group in &info.groups {
                println!(
                    "  {:<28} Zeile {:>5}, {:>4} Parameter, Prüfsumme {}",
                    group.label(),
                    group.line,
                    group.lines.len(),
                    group.checksum.as_deref().unwrap_or("FEHLT")
                );
            }
        }
        None => println!("Struktur: keine Parametergruppen (%MPGnn/%MDGnn) erkannt"),
    }
}

/// `param-check <datei>`: analysis and group structure (read-only).
pub fn param_check(args: &[String]) -> Result<ExitCode, String> {
    let (file, profile) = crate::file_and_profile(args)?;
    let data = read_file(&file)?;
    let analysis = analyze_parameters(&data, &profile);
    print_analysis(&file, &parameter_profile(&profile), &analysis);
    print_structure(parse_parameter_file(&data).as_ref());
    println!("Hinweis: Das Parameterformat der DIALOG 11 ist nicht verifiziert; Inhalte werden nicht bewertet.");
    Ok(if analysis.has_errors() { ExitCode::from(1) } else { ExitCode::SUCCESS })
}

/// `param-diff <alt> <neu>`: line comparison, plus format/version/checksum consistency.
pub fn param_diff(args: &[String]) -> Result<ExitCode, String> {
    let [old, new] = args else { return Err(USAGE.into()) };
    let (old, new) = (PathBuf::from(old), PathBuf::from(new));
    let (a, b) = (read_file(&old)?, read_file(&new)?);
    let diff = diff_parameters(&a, &b, &file_name(&old), &file_name(&new));
    println!("{}", diff.summary());
    if !diff.diff_text.is_empty() {
        println!("{}", diff.diff_text);
    }
    if let (Some(before), Some(after)) = (parse_parameter_file(&a), parse_parameter_file(&b)) {
        for problem in compatibility_problems(&before, &after) {
            println!("Sperre beim Schreiben: {problem}");
        }
    }
    Ok(if diff.identical() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn archive_for(options: &Options) -> ParameterArchive {
    ParameterArchive::new(Path::new(options.archive.as_deref().unwrap_or(".")))
}

fn wait_for_operator(options: &Options, instruction: &str) -> bool {
    eprintln!("{instruction}");
    if options.sim {
        eprintln!("SIMULATION - die simulierte CNC liefert immer dieselben Beispielparameter (eine Abweichung bei der Kontrolle ist daher zu erwarten).");
        return true;
    }
    eprint!("ENTER drücken, sobald die CNC bereit ist (q + ENTER bricht ab): ");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() != "q"
}

struct ReadOutcome {
    program: ReceivedProgram,
    stored: StoredParameterSet,
    port: String,
}

fn read_into_archive(options: &Options, profile: &Profile, archive: &ParameterArchive) -> Result<ReadOutcome, String> {
    let mut transport = make_transport(options, profile, sample_parameters())?;
    let port = transport.name();
    let clock = SystemClock::default();
    let token = CancelToken::new();
    let mut console = Console::default();
    let result = read_parameters(transport.as_mut(), profile, &clock, &token, &mut console);
    if result.raw.is_empty() {
        let mut message = format!("Keine Parameter empfangen ({})", result.end_reason.label());
        if let Some(error) = &result.error {
            message.push('\n');
            message.push_str(&error.as_text());
        }
        return Err(message);
    }
    let program = process_received(&result, &parameter_profile(profile));
    let stored = archive
        .store(&result, &program, profile)
        .map_err(|e| format!("Parameter speichern: {e}"))?
        .ok_or("Keine Parameter empfangen")?;
    let complete = stored.complete();
    println!(
        "Parameter gesichert: {} ({} Bytes, SHA-256 {}…){}",
        stored.path.display(),
        program.normalized.len(),
        stored.sha256().get(..16).unwrap_or_default(),
        if complete { "" } else { " - UNVOLLSTÄNDIG/UNGEPRÜFT" }
    );
    if let Some(info) = parse_parameter_file(&program.normalized) {
        println!("  {}", info.summary());
    }
    Ok(ReadOutcome { program, stored, port })
}

/// `param-read (--port|--tcp|--rfc2217|--sim) [--archiv DIR]`: store the CNC's parameter output.
pub fn param_read(args: &[String]) -> Result<ExitCode, String> {
    let options = parse_options(args)?;
    let profile = load_profile(options.profile.as_deref())?;
    if !options.positional.is_empty() {
        return Err(USAGE.into());
    }
    if !wait_for_operator(&options, "Parameter auslesen: Parameterausgabe an der CNC starten.") {
        eprintln!("Abgebrochen.");
        return Ok(ExitCode::from(1));
    }
    let outcome = read_into_archive(&options, &profile, &archive_for(&options))?;
    Ok(if outcome.stored.complete() { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn ask(prompt: &str, expected: &str) -> bool {
    eprint!("{prompt}");
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == expected
}

/// `param-write <datei> (--port ...|--sim) --freischalten [--archiv DIR]`.
pub fn param_write(args: &[String]) -> Result<ExitCode, String> {
    let options = parse_options(args)?;
    let profile = load_profile(options.profile.as_deref())?;
    let [file] = options.positional.as_slice() else { return Err(USAGE.into()) };
    let path = PathBuf::from(file);
    let candidate = ParameterCandidate::new(file_name(&path), read_file(&path)?, &profile);
    print_analysis(&path, &parameter_profile(&profile), &candidate.analysis);
    print_structure(candidate.structure().as_ref());

    // Locks that need no transfer are checked first - nothing is read or sent before.
    if !profile.parameters.allow_write {
        eprintln!(
            "Gesperrt: Profil {} erlaubt kein Schreiben von Maschinenparametern (parameters.allow_write).",
            profile.id
        );
        return Ok(ExitCode::from(1));
    }
    if !options.unlock {
        eprintln!("Gesperrt: Schreibfunktion für diese Sitzung nicht freigeschaltet (Option --freischalten).");
        return Ok(ExitCode::from(1));
    }
    if candidate.analysis.has_errors() || candidate.payload.is_none() {
        let reason = candidate.transform_error.clone().unwrap_or_else(|| "Parameterdatei enthält Prüffehler".into());
        eprintln!("Gesperrt: {reason}");
        return Ok(ExitCode::from(1));
    }
    eprintln!("\n{PARAMETER_WRITE_NOTICE}\n");
    if !ask("Schreibfunktion für diese Sitzung freischalten? (ja eintippen): ", "ja") {
        eprintln!("Nicht freigeschaltet - nichts geschrieben.");
        return Ok(ExitCode::from(1));
    }
    let archive = archive_for(&options);

    // Step 1: current backup from the machine (same run = same session).
    if !wait_for_operator(&options, "\nSchritt 1/3 - Sicherung: Parameterausgabe an der CNC starten.") {
        eprintln!("Abgebrochen - nichts geschrieben.");
        return Ok(ExitCode::from(1));
    }
    let backup_read = read_into_archive(&options, &profile, &archive)?;
    let clock = SystemClock::default();
    let backup = ParameterBackupInfo {
        path_name: file_name(&backup_read.stored.path),
        data: backup_read.program.normalized.clone(),
        read_at: clock.now(),
        profile_id: profile.id.clone(),
        port: backup_read.port.clone(),
        complete: backup_read.stored.complete(),
    };
    let gate = evaluate_parameter_write_gate(&GateInput {
        profile: &profile,
        session_unlocked: true,
        candidate: Some(&candidate),
        backup: Some(&backup),
        now: clock.now(),
        port_label: &backup_read.port,
        simulation: options.sim,
        port: &backup_read.port,
        busy: false,
    });
    if let Some(diff) = &gate.diff {
        println!("\nÄnderungen gegenüber der Sicherung: {}", diff.summary());
        println!("{}", diff.diff_text);
    }
    if !gate.allowed {
        eprintln!("\nParameter schreiben gesperrt:");
        for reason in &gate.reasons {
            eprintln!("  - {reason}");
        }
        return Ok(ExitCode::from(1));
    }

    // Step 2: typed confirmation immediately before the transfer.
    let payload = match &candidate.payload {
        Some(payload) => payload.clone(),
        None => prepare_parameter_payload(&candidate.data, &profile).map_err(|e| e.to_string())?,
    };
    let mode = if options.sim { "SIMULATION - keine Verbindung zur Maschine" } else { "REALE ÜBERTRAGUNG" };
    eprintln!(
        "\nSchritt 2/3 - Schreiben ({mode})\nAnschluss {}, Profil {} ({}), Protokoll {}\nDatei {} (SHA-256 {}…), Sicherung {}\n\
         Parametereingabe an der CNC freigeben (Schlüsselschalter/Schlüsselzahl) und Einlesen starten.",
        backup_read.port,
        profile.display_name,
        profile.verification_status.label(),
        profile.protocol.mode.label(),
        candidate.name,
        candidate.analysis.sha256.get(..16).unwrap_or_default(),
        backup.path_name
    );
    if !ask(
        &format!("Zum Schreiben {WRITE_CONFIRMATION_PHRASE} eintippen (alles andere bricht ab): "),
        WRITE_CONFIRMATION_PHRASE,
    ) {
        eprintln!("Parameter wurden nicht geschrieben.");
        return Ok(ExitCode::from(1));
    }
    let mut transport = make_transport(&options, &profile, Vec::new())?;
    let token = CancelToken::new();
    let mut console = Console::default();
    let result = write_parameters(transport.as_mut(), &profile, &payload, &clock, &token, &mut console);
    drop(transport);
    if result.state != SendState::Completed {
        eprintln!("{}", result.state.label());
        if let Some(error) = &result.error {
            eprintln!("{}", error.as_text());
        }
        return Ok(ExitCode::from(1));
    }
    println!(
        "Parameter gesendet ({} Bytes). Die Sicherung entspricht nicht mehr dem Maschinenstand.",
        result.bytes_sent
    );

    // Step 3: verification by reading again.
    if !wait_for_operator(&options, "\nSchritt 3/3 - Kontrolle: Parameterausgabe an der CNC erneut starten.") {
        eprintln!("KONTROLLE NICHT DURCHGEFÜHRT - Parameter vor dem Verfahren unbedingt auslesen und vergleichen!");
        return Ok(ExitCode::from(1));
    }
    let check_read = read_into_archive(&options, &profile, &archive)?;
    let check = diff_parameters(&candidate.data, &check_read.program.normalized, "geschrieben", "ausgelesen");
    if check.identical() {
        println!("Kontrolle: ausgelesene Parameter stimmen mit der geschriebenen Datei überein.");
        Ok(ExitCode::SUCCESS)
    } else {
        println!("Kontrolle: ABWEICHUNG zur geschriebenen Datei - {}", check.summary());
        println!("{}", check.diff_text);
        Ok(ExitCode::from(1))
    }
}

fn structure_json(info: &ParameterFileInfo) -> Value {
    let mut header: Vec<(&String, &String)> = info.header.iter().collect();
    header.sort();
    json!({
        "kind": info.kind.map(|k| k.letter().to_string()),
        "groups": info.groups.iter().map(|g| json!({
            "key": g.key(), "label": g.label(), "line": g.line, "lines": g.lines, "checksum": g.checksum,
        })).collect::<Vec<_>>(),
        "header": header.into_iter().map(|(k, v)| vec![k.clone(), v.clone()]).collect::<Vec<_>>(),
        "eot": info.eot,
        "summary": info.summary(),
        "software_versions": info.software_versions(),
        "machine_type": info.machine_type(),
        "machine_number": info.machine_number(),
        "date": info.date(),
        "findings": info.findings.iter().map(crate::finding_value).collect::<Vec<_>>(),
    })
}

/// Hidden: `param-json <profil.json> <alt> <neu>` - machine readable results for the parity test.
pub fn param_json(args: &[String]) -> Result<ExitCode, String> {
    let [profile_path, old, new] = args else { return Err(USAGE.into()) };
    let text = std::fs::read_to_string(profile_path).map_err(|e| e.to_string())?;
    let (profile, _) = parse_profile_text(&text, profile_path, false).map_err(|e| e.to_string())?;
    let (a, b) = (read_file(Path::new(old))?, read_file(Path::new(new))?);
    let analysis = analyze_parameters(&b, &profile);
    let diff = diff_parameters(&a, &b, "alt", "neu");
    let (before, after) = (parse_parameter_file(&a), parse_parameter_file(&b));
    let problems = match (&before, &after) {
        (Some(x), Some(y)) => Some(compatibility_problems(x, y)),
        _ => None,
    };
    let payload = match prepare_parameter_payload(&b, &profile) {
        Ok(p) => json!({"ok": true, "data": crate::hex(&p.data), "applied": p.applied}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    };
    let out = json!({
        "findings": analysis.findings.iter().map(crate::finding_value).collect::<Vec<_>>(),
        "structure": after.as_ref().map(structure_json),
        "diff": {
            "changed": diff.changed_lines, "added": diff.added_lines, "removed": diff.removed_lines,
            "text": diff.diff_text, "truncated": diff.truncated,
        },
        "problems": problems,
        "payload": payload,
    });
    println!("{out}");
    Ok(ExitCode::SUCCESS)
}
