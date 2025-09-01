use std::{marker::PhantomData, process::Stdio};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines},
    process::{ChildStderr, ChildStdin, ChildStdout, Command},
    sync::mpsc,
    task::JoinHandle,
};

/// Parse a pipeline string into individual commands
///
/// Takes a pipeline string and splits it by `|` characters that are not inside quotes,
/// trims whitespace from each command, filters out empty commands, and returns a Vec<String> of valid commands.
///
/// # Arguments
/// * `pipeline` - The pipeline string to parse (e.g., "ls -l | grep pattern" or "echo 'a|b' | cat")
///
/// # Returns
/// * `Ok(Vec<String>)` - Vector of trimmed, non-empty commands
/// * `Err(anyhow::Error)` - If no valid commands are found
///
/// # Examples
/// ```
/// let commands = parse_pipeline("ls -l | grep txt | head -5")?;
/// assert_eq!(commands, vec!["ls -l", "grep txt", "head -5"]);
///
/// let commands = parse_pipeline("echo 'a|b' | cat")?;
/// assert_eq!(commands, vec!["echo 'a|b'", "cat"]);
/// ```
pub fn parse_pipeline(pipeline: &str) -> anyhow::Result<Vec<String>> {
    let commands = split_pipeline_with_quotes(pipeline)?;

    if commands.is_empty() {
        anyhow::bail!("No valid commands found in pipeline: '{}'", pipeline);
    }

    Ok(commands)
}

/// Split a pipeline string by pipe characters, respecting quoted sections
fn split_pipeline_with_quotes(pipeline: &str) -> anyhow::Result<Vec<String>> {
    let mut commands = Vec::new();
    let mut current_command = String::new();
    let chars = pipeline.chars();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escape_next = false;

    for ch in chars {
        if escape_next {
            current_command.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => {
                current_command.push(ch);
                escape_next = true;
            }
            '\'' if !in_double_quote => {
                current_command.push(ch);
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                current_command.push(ch);
                in_double_quote = !in_double_quote;
            }
            '|' if !in_single_quote && !in_double_quote => {
                let trimmed = current_command.trim().to_string();
                if !trimmed.is_empty() {
                    commands.push(trimmed);
                }
                current_command.clear();
            }
            _ => {
                current_command.push(ch);
            }
        }
    }

    let trimmed = current_command.trim().to_string();
    if !trimmed.is_empty() {
        commands.push(trimmed);
    }

    if in_single_quote || in_double_quote {
        anyhow::bail!("Unclosed quote in pipeline: '{}'", pipeline);
    }

    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_pipeline() {
        let result = parse_pipeline("ls -l | grep txt").unwrap();
        assert_eq!(result, vec!["ls -l", "grep txt"]);
    }

    #[test]
    fn test_quoted_pipe_single_quotes() {
        let result = parse_pipeline("echo '[]' | jq '. | length'").unwrap();
        assert_eq!(result, vec!["echo '[]'", "jq '. | length'"]);
    }

    #[test]
    fn test_quoted_pipe_double_quotes() {
        let result = parse_pipeline(r#"echo "test|value" | grep test"#).unwrap();
        assert_eq!(result, vec![r#"echo "test|value""#, "grep test"]);
    }

    #[test]
    fn test_multiple_pipes() {
        let result = parse_pipeline("echo hello | cat | wc -l").unwrap();
        assert_eq!(result, vec!["echo hello", "cat", "wc -l"]);
    }

    #[test]
    fn test_multiple_quoted_pipes() {
        let result = parse_pipeline(r#"echo 'a|b|c' | tr '|' '\n'"#).unwrap();
        assert_eq!(result, vec!["echo 'a|b|c'", r#"tr '|' '\n'"#]);
    }

    #[test]
    fn test_escaped_quotes() {
        let result = parse_pipeline(r#"echo 'it'\''s a test' | cat"#).unwrap();
        assert_eq!(result, vec![r#"echo 'it'\''s a test'"#, "cat"]);
    }

    #[test]
    fn test_unclosed_single_quote() {
        let result = parse_pipeline("echo 'unclosed | cat");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unclosed quote"));
    }

    #[test]
    fn test_unclosed_double_quote() {
        let result = parse_pipeline(r#"echo "unclosed | cat"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Unclosed quote"));
    }

    #[test]
    fn test_empty_pipeline() {
        let result = parse_pipeline("");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No valid commands found")
        );
    }

    #[test]
    fn test_only_pipes() {
        let result = parse_pipeline("|||");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No valid commands found")
        );
    }
}

pub trait StageKind {}

pub struct Head;
impl StageKind for Head {}

pub struct Pipe;
impl StageKind for Pipe {}

pub struct Stage<S: StageKind> {
    waiter: JoinHandle<()>,
    _marker: PhantomData<S>,
}

fn parse_command(cmd: &str) -> anyhow::Result<Command> {
    let parts = shlex::split(cmd.trim())
        .ok_or_else(|| anyhow::anyhow!("Failed to parse {}: invalid shell syntax", cmd))?;

    if parts.is_empty() {
        return Err(anyhow::anyhow!("The command is empty"));
    }

    let mut command = Command::new(&parts[0]);
    for arg in parts.iter().skip(1) {
        command.arg(arg);
    }
    Ok(command)
}

#[allow(clippy::type_complexity)]
fn setup_command(
    mut command: Command,
    use_stdin: bool,
) -> anyhow::Result<(
    Option<BufWriter<ChildStdin>>,
    Lines<BufReader<ChildStdout>>,
    Lines<BufReader<ChildStderr>>,
)> {
    let stdin_config = if use_stdin {
        Stdio::piped()
    } else {
        Stdio::null()
    };

    let mut child = match command
        .stdin(stdin_config)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::bail!("Command {:?} is not found", command.as_std().get_program());
            }
            return Err(e.into());
        }
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("stdout is not available"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("stderr is not available"))?;

    Ok((
        if use_stdin {
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("stdin is not available"))?;
            Some(BufWriter::new(stdin))
        } else {
            None
        },
        BufReader::new(stdout).lines(),
        BufReader::new(stderr).lines(),
    ))
}

fn spawn_process_output(
    mut stdout_reader: Lines<BufReader<ChildStdout>>,
    mut stderr_reader: Lines<BufReader<ChildStderr>>,
    tx: mpsc::Sender<String>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(Some(out)) = stdout_reader.next_line() => {
                    // Remove ANSI escape sequences and properly decode the byte array as UTF-8 string
                    let stripped = strip_ansi_escapes::strip(&out);
                    let decoded = String::from_utf8_lossy(&stripped).into_owned();
                    let _ = tx.send(decoded).await;
                },
                Ok(Some(err)) = stderr_reader.next_line() => {
                    let _ = tx.send(err).await;
                },
                else => {
                    // NOTE: BufReader will be closed when the command is terminated.
                    // Without a return here, all outputs may not be rendered correctly.
                    // (they may not display properly unless the Enter key is pressed repeatedly)
                    return;
                }
            }
        }
    })
}

impl Stage<Head> {
    pub fn spawn(cmd: &str, tx: mpsc::Sender<String>) -> anyhow::Result<Self> {
        let command = parse_command(cmd)?;
        let (_, stdout_reader, stderr_reader) = setup_command(command, false)?;

        Ok(Self {
            waiter: spawn_process_output(stdout_reader, stderr_reader, tx),
            _marker: PhantomData,
        })
    }

    pub fn abort_if_running(&mut self) {
        self.waiter.abort();
    }
}

impl Stage<Pipe> {
    pub fn spawn(
        cmd: &str,
        mut rx: mpsc::Receiver<String>,
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<Self> {
        let command = parse_command(cmd)?;
        let (stdin_writer, stdout_reader, stderr_reader) = setup_command(command, true)?;
        let mut stdin_writer = stdin_writer.expect("stdin should be available for Pipe stage");

        let waiter = tokio::spawn(async move {
            let input_task = tokio::spawn(async move {
                while let Some(line) = rx.recv().await {
                    let _ = stdin_writer
                        .write_all(format!("{}\n", line).as_bytes())
                        .await;
                    let _ = stdin_writer.flush().await;
                }
                let _ = stdin_writer.flush().await;
            });

            let output_task = spawn_process_output(stdout_reader, stderr_reader, tx);

            let _ = tokio::join!(input_task, output_task);
        });

        Ok(Self {
            waiter,
            _marker: PhantomData,
        })
    }

    pub fn abort_if_running(&mut self) {
        self.waiter.abort();
    }
}

pub struct Pipeline {
    head: Option<Stage<Head>>,
    pipes: Vec<Stage<Pipe>>,
}

impl Pipeline {
    pub fn spawn(cmds: Vec<String>, tx: mpsc::Sender<String>) -> anyhow::Result<Self> {
        if cmds.is_empty() {
            return Err(anyhow::anyhow!("No commands provided"));
        }

        let mut pipeline = Self {
            head: None,
            pipes: Vec::new(),
        };

        if cmds.len() == 1 {
            let head = Stage::<Head>::spawn(&cmds[0], tx)?;
            pipeline.head = Some(head);
            return Ok(pipeline);
        }

        let (prev_tx, mut prev_rx) = mpsc::channel::<String>(100);

        let head = Stage::<Head>::spawn(&cmds[0], prev_tx)?;
        pipeline.head = Some(head);

        for cmd in cmds.iter().take(cmds.len() - 1).skip(1) {
            let (next_tx, next_rx) = mpsc::channel::<String>(100);
            let tx_clone = next_tx.clone();
            let pipe = Stage::<Pipe>::spawn(cmd, prev_rx, tx_clone)?;
            pipeline.pipes.push(pipe);
            prev_rx = next_rx;
        }

        let last_pipe = Stage::<Pipe>::spawn(&cmds[cmds.len() - 1], prev_rx, tx)?;
        pipeline.pipes.push(last_pipe);

        Ok(pipeline)
    }

    pub fn abort_all(&mut self) {
        if let Some(head) = &mut self.head {
            head.abort_if_running();
        }
        for pipe in &mut self.pipes {
            pipe.abort_if_running();
        }
    }
}
