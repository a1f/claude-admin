use std::process::Command;

fn main() -> anyhow::Result<()> {
    println!("Scanning all tmux panes for Claude sessions...\n");

    // Check if tmux is running
    let check = Command::new("tmux")
        .args(["list-sessions"])
        .output()?;

    if !check.status.success() {
        println!("tmux is not running");
        return Ok(());
    }

    // List all panes with their current process
    let output = Command::new("tmux")
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{window_index}\t#{pane_index}\t#{pane_id}\t#{pane_current_command}\t#{pane_current_path}",
        ])
        .output()?;

    if !output.status.success() {
        eprintln!("Failed to list panes: {}", String::from_utf8_lossy(&output.stderr));
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    println!("{:<15} {:>5} {:>5} {:>6} {:<15} {}",
        "SESSION", "WIN", "PANE", "ID", "PROCESS", "WORKING DIR");
    println!("{}", "-".repeat(80));

    let mut claude_locations = Vec::new();

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 6 {
            let session = parts[0];
            let window = parts[1];
            let pane = parts[2];
            let pane_id = parts[3];
            let process = parts[4];
            let path = parts[5];

            // Check if this is a Claude process
            // Claude shows its version as the process name (e.g., "2.1.20")
            let looks_like_version = process.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
                && process.contains('.');
            let is_claude = process.to_lowercase().contains("claude")
                || process == "node"  // Claude often shows as node
                || process == "deno"  // Or deno
                || looks_like_version;

            let marker = if is_claude { ">>> CLAUDE <<<" } else { "" };

            println!("{:<15} {:>5} {:>5} {:>6} {:<15} {}",
                session, window, pane, pane_id, process, path);

            if is_claude {
                claude_locations.push((session.to_string(), window.to_string(), pane.to_string(), pane_id.to_string(), path.to_string()));
            }

            if !marker.is_empty() {
                println!("                                      {}", marker);
            }
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("\nClaude Sessions Found: {}", claude_locations.len());

    for (i, (session, window, pane, pane_id, path)) in claude_locations.iter().enumerate() {
        println!("\n  {}. {}:{}.{} ({})", i + 1, session, window, pane, pane_id);
        println!("     Working dir: {}", path);

        // Capture last few lines of pane content
        let capture = Command::new("tmux")
            .args(["capture-pane", "-p", "-t", pane_id, "-S", "-5"])
            .output();

        if let Ok(capture) = capture {
            if capture.status.success() {
                let content = String::from_utf8_lossy(&capture.stdout);
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    println!("     Last output:");
                    for line in trimmed.lines().take(5) {
                        println!("       | {}", line);
                    }
                }
            }
        }
    }

    Ok(())
}
