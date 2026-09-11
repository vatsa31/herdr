use std::collections::HashMap;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::env;
use super::manifest::{effective_platforms, ensure_platform_supported};
use super::plugin_manifest_available;
use super::super::responses::{encode_error, encode_success};
use crate::api::schema::{
    InstalledPluginInfo, PluginCommandLogInfo, PluginCommandStatus, PluginManifestResource,
    PluginResourceActivateParams, PluginResourceCollection, PluginResourceItem,
    PluginResourceListParams, PluginResourceTarget, ResponseResult,
};
use crate::app::App;
use crate::events::AppEvent;

pub(crate) const PLUGIN_RESOURCE_SCHEMA: &str = "herdr.plugin.resource/v1";
const PLUGIN_RESOURCE_OUTPUT_MAX_BYTES: usize = 256 * 1024;
const PLUGIN_RESOURCE_TIMEOUT: Duration = Duration::from_secs(25);
const DEFAULT_REFRESH_SECS: u64 = 60;
const MAX_CONSECUTIVE_FAILURES: u32 = 8;

#[derive(Debug, Clone)]
pub(crate) struct PluginResourceJob {
    plugin_id: String,
    resource: PluginManifestResource,
    plugin_root: String,
    generation: u64,
    in_flight: bool,
    queued: bool,
    next_due: Instant,
    consecutive_failures: u32,
    had_success: bool,
    snapshot: PluginResourceCollection,
}

impl PluginResourceJob {
    fn new(plugin: &InstalledPluginInfo, resource: PluginManifestResource, now: Instant) -> Self {
        let snapshot = PluginResourceCollection {
            plugin_id: plugin.plugin_id.clone(),
            resource_id: resource.id.clone(),
            label: resource.title.clone(),
            items: Vec::new(),
            matched_count: None,
            truncated: false,
            fetched_unix_ms: None,
            loading: true,
            refreshing: false,
            error: None,
            stale: false,
        };
        Self {
            plugin_id: plugin.plugin_id.clone(),
            resource,
            plugin_root: plugin.plugin_root.clone(),
            generation: 0,
            in_flight: false,
            queued: false,
            next_due: now,
            consecutive_failures: 0,
            had_success: false,
            snapshot,
        }
    }
}

#[derive(serde::Deserialize)]
struct ProviderStdout {
    schema: Option<String>,
    plugin_id: Option<String>,
    resource_id: Option<String>,
    label: Option<String>,
    #[serde(default)]
    items: Vec<ProviderItem>,
    matched_count: Option<u64>,
    #[serde(default)]
    truncated: bool,
    fetched_unix_ms: Option<u64>,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct ProviderItem {
    id: Option<String>,
    #[serde(default)]
    primary: String,
    #[serde(default)]
    secondary: String,
    payload: Option<String>,
}

pub(crate) fn sanitize_plugin_display_text(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn job_key(plugin_id: &str, resource_id: &str) -> String {
    format!("{plugin_id}\n{resource_id}")
}

impl App {
    pub(crate) fn sync_plugin_resources(&mut self, now: Instant) {
        let mut desired = HashMap::new();
        let mut plugins = self
            .state
            .installed_plugins
            .values()
            .filter(|plugin| plugin.enabled && plugin_manifest_available(plugin))
            .cloned()
            .collect::<Vec<_>>();
        plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
        for plugin in plugins {
            for resource in plugin.resources.clone() {
                if ensure_platform_supported(
                    &effective_platforms(&resource.platforms, &plugin.platforms),
                    "resource",
                )
                .is_err()
                {
                    continue;
                }
                desired.insert(job_key(&plugin.plugin_id, &resource.id), (plugin.clone(), resource));
            }
        }

        self.plugin_resource_jobs
            .retain(|key, _| desired.contains_key(key));
        for (key, (plugin, resource)) in desired {
            self.plugin_resource_jobs
                .entry(key)
                .and_modify(|job| {
                    job.resource = resource.clone();
                    job.plugin_root = plugin.plugin_root.clone();
                    if !job.had_success {
                        job.snapshot.label = resource.title.clone();
                    }
                })
                .or_insert_with(|| PluginResourceJob::new(&plugin, resource, now));
        }
        self.publish_plugin_resources();
        self.tick_plugin_resources(now);
    }

    pub(crate) fn next_plugin_resource_deadline(&self) -> Option<Instant> {
        self.plugin_resource_jobs
            .values()
            .filter(|job| !job.in_flight)
            .map(|job| job.next_due)
            .min()
    }

    pub(crate) fn tick_plugin_resources(&mut self, now: Instant) -> bool {
        let due = self
            .plugin_resource_jobs
            .iter()
            .filter(|(_, job)| !job.in_flight && now >= job.next_due)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if due.is_empty() {
            return false;
        }
        for key in due {
            self.start_resource_fetch(&key, now, false);
        }
        true
    }

    fn start_resource_fetch(&mut self, key: &str, now: Instant, manual: bool) {
        let Some(job) = self.plugin_resource_jobs.get_mut(key) else {
            return;
        };
        if job.in_flight {
            job.queued = true;
            if manual {
                job.snapshot.refreshing = job.had_success;
            }
            self.publish_plugin_resources();
            return;
        }
        let Some(plugin) = self.state.installed_plugins.get(&job.plugin_id).cloned() else {
            return;
        };
        if let Err(err) = env::ensure_plugin_user_dirs(&plugin) {
            job.snapshot.error = Some(err.to_string());
            job.snapshot.loading = false;
            job.snapshot.refreshing = false;
            job.snapshot.stale = job.had_success;
            job.next_due = now + backoff(job.consecutive_failures);
            self.publish_plugin_resources();
            return;
        }
        job.generation = job.generation.saturating_add(1);
        job.in_flight = true;
        job.queued = false;
        job.snapshot.loading = !job.had_success;
        job.snapshot.refreshing = job.had_success;
        let generation = job.generation;
        let plugin_id = job.plugin_id.clone();
        let resource_id = job.resource.id.clone();
        let command = job.resource.command.clone();
        let plugin_root = std::path::PathBuf::from(&job.plugin_root);
        let mut env_vars = env::plugin_path_env(&plugin);
        env_vars.extend([
            (
                crate::api::SOCKET_PATH_ENV_VAR.to_string(),
                crate::api::socket_path().display().to_string(),
            ),
            ("HERDR_ENV".to_string(), "1".to_string()),
            ("HERDR_PLUGIN_ID".to_string(), plugin.plugin_id.clone()),
            (
                "HERDR_PLUGIN_RESOURCE_ID".to_string(),
                resource_id.clone(),
            ),
        ]);
        if let Ok(current_exe) = std::env::current_exe() {
            env_vars.push((
                "HERDR_BIN_PATH".to_string(),
                current_exe.display().to_string(),
            ));
        }
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = run_resource_command(&command, &plugin_root, env_vars);
            let _ = event_tx.blocking_send(AppEvent::PluginResourceFinished {
                plugin_id,
                resource_id,
                generation,
                result,
            });
        });
        self.publish_plugin_resources();
    }

    pub(crate) fn handle_plugin_resource_finished(
        &mut self,
        plugin_id: String,
        resource_id: String,
        generation: u64,
        result: Result<PluginResourceCollection, String>,
        now: Instant,
    ) {
        let key = job_key(&plugin_id, &resource_id);
        let Some(job) = self.plugin_resource_jobs.get_mut(&key) else {
            return;
        };
        if job.generation != generation {
            return;
        }
        job.in_flight = false;
        match result {
            Ok(mut collection) => {
                collection.plugin_id = plugin_id;
                collection.resource_id = resource_id.clone();
                if collection.label.trim().is_empty() {
                    collection.label = job.resource.title.clone();
                }
                collection.label = sanitize_plugin_display_text(&collection.label);
                for item in &mut collection.items {
                    item.primary = sanitize_plugin_display_text(&item.primary);
                    item.secondary = sanitize_plugin_display_text(&item.secondary);
                }
                collection.loading = false;
                collection.refreshing = false;
                collection.stale = collection.error.is_some();
                if let Some(error) = collection.error.as_ref() {
                    job.consecutive_failures = job.consecutive_failures.saturating_add(1);
                    if job.had_success {
                        job.snapshot.error = Some(error.clone());
                        job.snapshot.stale = true;
                        job.snapshot.loading = false;
                        job.snapshot.refreshing = false;
                    } else {
                        job.snapshot = collection;
                    }
                } else {
                    job.had_success = true;
                    job.consecutive_failures = 0;
                    job.snapshot = collection;
                    job.snapshot.error = None;
                    job.snapshot.stale = false;
                }
                job.next_due = now + Duration::from_secs(job.resource.refresh_secs.max(5));
            }
            Err(error) => {
                job.consecutive_failures = job.consecutive_failures.saturating_add(1);
                job.snapshot.loading = false;
                job.snapshot.refreshing = false;
                job.snapshot.error = Some(error);
                job.snapshot.stale = job.had_success;
                job.next_due = now + backoff(job.consecutive_failures);
            }
        }
        let queued = job.queued;
        if queued {
            job.queued = false;
            job.next_due = now;
        }
        self.publish_plugin_resources();
        if queued {
            self.start_resource_fetch(&key, now, false);
        }
    }

    fn publish_plugin_resources(&mut self) {
        let mut resources = self
            .plugin_resource_jobs
            .values()
            .map(|job| job.snapshot.clone())
            .collect::<Vec<_>>();
        resources.sort_by(|a, b| {
            a.plugin_id
                .cmp(&b.plugin_id)
                .then(a.resource_id.cmp(&b.resource_id))
        });
        self.state.plugin_resources = resources;
    }

    fn resource_snapshot(
        &self,
        plugin_id: &str,
        resource_id: &str,
    ) -> Option<PluginResourceCollection> {
        self.plugin_resource_jobs
            .get(&job_key(plugin_id, resource_id))
            .map(|job| job.snapshot.clone())
    }

    pub(crate) fn handle_plugin_resource_list(
        &mut self,
        id: String,
        params: PluginResourceListParams,
    ) -> String {
        self.sync_plugin_resources(Instant::now());
        let mut resources = self.state.plugin_resources.clone();
        if let Some(plugin_id) = params.plugin_id.as_deref() {
            resources.retain(|resource| resource.plugin_id == plugin_id);
        }
        encode_success(id, ResponseResult::PluginResourceList { resources })
    }

    pub(crate) fn handle_plugin_resource_refresh(
        &mut self,
        id: String,
        params: PluginResourceTarget,
    ) -> String {
        let now = Instant::now();
        self.sync_plugin_resources(now);
        let key = job_key(&params.plugin_id, &params.resource_id);
        if !self.plugin_resource_jobs.contains_key(&key) {
            return encode_error(id, "plugin_resource_not_found", "plugin resource not found");
        }
        self.start_resource_fetch(&key, now, true);
        let Some(resource) = self.resource_snapshot(&params.plugin_id, &params.resource_id) else {
            return encode_error(id, "plugin_resource_not_found", "plugin resource not found");
        };
        encode_success(id, ResponseResult::PluginResourceRefreshed { resource })
    }

    pub(crate) fn handle_plugin_resource_activate(
        &mut self,
        id: String,
        params: PluginResourceActivateParams,
    ) -> String {
        let now = Instant::now();
        self.sync_plugin_resources(now);
        let key = job_key(&params.plugin_id, &params.resource_id);
        let Some(job) = self.plugin_resource_jobs.get(&key) else {
            return encode_error(id, "plugin_resource_not_found", "plugin resource not found");
        };
        let Some(item) = job
            .snapshot
            .items
            .iter()
            .find(|item| item.id == params.item_id)
            .cloned()
        else {
            return encode_error(id, "plugin_resource_item_not_found", "plugin resource item not found");
        };
        if job.resource.activate_command.is_empty() {
            return encode_error(
                id,
                "plugin_resource_activate_unavailable",
                "plugin resource does not declare an activate command",
            );
        }
        let Some(plugin) = self.state.installed_plugins.get(&params.plugin_id).cloned() else {
            return encode_error(id, "plugin_not_found", "plugin not found");
        };
        if !plugin.enabled {
            return encode_error(
                id,
                "plugin_disabled",
                format!("plugin {} is disabled", plugin.plugin_id),
            );
        }
        let context = self.current_plugin_context("plugin.resource.activate");
        let mut command = job.resource.activate_command.clone();
        let payload = item.payload.clone().unwrap_or_else(|| item.id.clone());
        let log = match self.start_plugin_resource_activate(
            &plugin,
            command.split_off(0),
            &context,
            &params.resource_id,
            &item.id,
            &payload,
        ) {
            Ok(log) => log,
            Err((code, message)) => return encode_error(id, code, message),
        };
        encode_success(
            id,
            ResponseResult::PluginResourceActivated {
                plugin_id: params.plugin_id,
                resource_id: params.resource_id,
                item_id: params.item_id,
                log,
            },
        )
    }

    fn start_plugin_resource_activate(
        &mut self,
        plugin: &InstalledPluginInfo,
        command: Vec<String>,
        context: &crate::api::schema::PluginInvocationContext,
        resource_id: &str,
        item_id: &str,
        payload: &str,
    ) -> Result<PluginCommandLogInfo, (&'static str, String)> {
        let Some(program) = command.first().cloned() else {
            return Err((
                "invalid_plugin_command",
                "command must not be empty".to_string(),
            ));
        };
        let args = command.iter().skip(1).cloned().collect::<Vec<_>>();
        super::env::ensure_plugin_user_dirs(plugin)
            .map_err(|err| ("plugin_user_dir_create_failed", err.to_string()))?;
        let log_id = format!("plugin-log-{}", self.state.next_plugin_command_log_id);
        self.state.next_plugin_command_log_id += 1;
        let started_unix_ms = current_unix_ms();
        let context_json = serde_json::to_string(context)
            .map_err(|err| ("invalid_plugin_context", err.to_string()))?;
        let mut env = super::env::plugin_path_env(plugin);
        env.extend([
            (
                crate::api::SOCKET_PATH_ENV_VAR.to_string(),
                crate::api::socket_path().display().to_string(),
            ),
            ("HERDR_ENV".to_string(), "1".to_string()),
            ("HERDR_PLUGIN_ID".to_string(), plugin.plugin_id.clone()),
            ("HERDR_PLUGIN_CONTEXT_JSON".to_string(), context_json),
            (
                "HERDR_PLUGIN_RESOURCE_ID".to_string(),
                resource_id.to_string(),
            ),
            ("HERDR_PLUGIN_ITEM_ID".to_string(), item_id.to_string()),
            (
                "HERDR_PLUGIN_ITEM_PAYLOAD".to_string(),
                payload.to_string(),
            ),
        ]);
        if let Ok(current_exe) = std::env::current_exe() {
            env.push((
                "HERDR_BIN_PATH".to_string(),
                current_exe.display().to_string(),
            ));
        }
        let plugin_root = std::path::PathBuf::from(&plugin.plugin_root);
        let log = PluginCommandLogInfo {
            log_id: log_id.clone(),
            plugin_id: plugin.plugin_id.clone(),
            action_id: Some("resource.activate".into()),
            event: None,
            command: command.clone(),
            status: PluginCommandStatus::Running,
            started_unix_ms,
            finished_unix_ms: None,
            exit_code: None,
            stdout: None,
            stderr: None,
            error: None,
        };
        self.state.plugin_command_logs.push(log.clone());
        self.state.plugin_commands_in_flight += 1;
        let event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let child =
                crate::plugin_command::command_for_argv_in_dir(&program, &args, &plugin_root)
                    .envs(env)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();
            let finished = match child {
                Ok(mut child) => {
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let stdout_reader = stdout.map(|stdout| {
                        std::thread::spawn(move || {
                            read_capped(stdout, PLUGIN_RESOURCE_OUTPUT_MAX_BYTES)
                        })
                    });
                    let stderr_reader = stderr.map(|stderr| {
                        std::thread::spawn(move || {
                            read_capped(stderr, PLUGIN_RESOURCE_OUTPUT_MAX_BYTES)
                        })
                    });
                    match child.wait() {
                        Ok(status) => AppEvent::PluginCommandFinished {
                            log_id,
                            finished_unix_ms: current_unix_ms(),
                            exit_code: status.code(),
                            stdout: stdout_reader
                                .and_then(|reader| reader.join().ok())
                                .unwrap_or_default(),
                            stderr: stderr_reader
                                .and_then(|reader| reader.join().ok())
                                .unwrap_or_default(),
                            error: None,
                        },
                        Err(err) => AppEvent::PluginCommandFinished {
                            log_id,
                            finished_unix_ms: current_unix_ms(),
                            exit_code: None,
                            stdout: String::new(),
                            stderr: String::new(),
                            error: Some(err.to_string()),
                        },
                    }
                }
                Err(err) => AppEvent::PluginCommandFinished {
                    log_id,
                    finished_unix_ms: current_unix_ms(),
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    error: Some(err.to_string()),
                },
            };
            let _ = event_tx.blocking_send(finished);
        });
        Ok(log)
    }
}

fn read_capped(mut reader: impl Read, cap: usize) -> String {
    let mut kept = Vec::with_capacity(cap.min(8192));
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let remaining = cap.saturating_sub(kept.len());
                if remaining == 0 {
                    continue;
                }
                kept.extend_from_slice(&buf[..n.min(remaining)]);
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

fn backoff(failures: u32) -> Duration {
    let capped = failures.min(MAX_CONSECUTIVE_FAILURES);
    let secs = DEFAULT_REFRESH_SECS.saturating_mul(1u64 << capped.min(5));
    Duration::from_secs(secs.min(300))
}

fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn run_resource_command(
    command: &[String],
    plugin_root: &std::path::Path,
    env: Vec<(String, String)>,
) -> Result<PluginResourceCollection, String> {
    let Some(program) = command.first() else {
        return Err("resource command must not be empty".into());
    };
    let args = &command[1..];
    let mut child = crate::plugin_command::command_for_argv_in_dir(program, args, plugin_root)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to start resource command: {err}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child
                    .stdout
                    .take()
                    .map(|stdout| read_capped(stdout, PLUGIN_RESOURCE_OUTPUT_MAX_BYTES))
                    .unwrap_or_default();
                let stderr = child
                    .stderr
                    .take()
                    .map(|stderr| read_capped(stderr, 8 * 1024))
                    .unwrap_or_default();
                if !status.success() {
                    let detail = if stderr.trim().is_empty() {
                        format!("resource command exited {}", status)
                    } else {
                        format!(
                            "resource command exited {}: {}",
                            status,
                            stderr.chars().take(240).collect::<String>()
                        )
                    };
                    return Err(detail);
                }
                return parse_provider_stdout(&stdout);
            }
            Ok(None) if started.elapsed() >= PLUGIN_RESOURCE_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("resource command timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(err) => return Err(format!("failed to wait for resource command: {err}")),
        }
    }
}

pub(crate) fn parse_provider_stdout(stdout: &str) -> Result<PluginResourceCollection, String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err("resource command produced no output".into());
    }
    let parsed: ProviderStdout =
        serde_json::from_str(trimmed).map_err(|err| format!("malformed resource JSON: {err}"))?;
    if parsed.schema.as_deref() != Some(PLUGIN_RESOURCE_SCHEMA) {
        return Err(format!(
            "unsupported resource schema {}",
            parsed.schema.unwrap_or_else(|| "missing".into())
        ));
    }
    let mut items = Vec::new();
    for item in parsed.items {
        let Some(id) = item
            .id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        items.push(PluginResourceItem {
            payload: item.payload.filter(|payload| !payload.trim().is_empty()),
            id,
            primary: item.primary,
            secondary: item.secondary,
        });
    }
    Ok(PluginResourceCollection {
        plugin_id: parsed.plugin_id.unwrap_or_default(),
        resource_id: parsed.resource_id.unwrap_or_default(),
        label: parsed.label.unwrap_or_default(),
        items,
        matched_count: parsed.matched_count,
        truncated: parsed.truncated,
        fetched_unix_ms: parsed.fetched_unix_ms.or(Some(current_unix_ms())),
        loading: false,
        refreshing: false,
        error: parsed
            .error
            .map(|error| error.trim().to_string())
            .filter(|error| !error.is_empty()),
        stale: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_control_characters() {
        assert_eq!(
            sanitize_plugin_display_text("PROJ-1\u{1b}[31m red\nnext"),
            "PROJ-1 [31m red next"
        );
    }

    #[test]
    fn parse_provider_stdout_requires_schema() {
        let err = parse_provider_stdout(r#"{"items":[]}"#).unwrap_err();
        assert!(err.contains("unsupported resource schema"));
    }

    #[test]
    fn parse_provider_stdout_reads_items() {
        let collection = parse_provider_stdout(
            r#"{
                "schema": "herdr.plugin.resource/v1",
                "plugin_id": "herdr-jira",
                "resource_id": "my-issues",
                "label": "My Jira issues",
                "items": [{"id":"PROJ-1","primary":"Fix login","secondary":"In Progress","payload":"PROJ-1"}],
                "matched_count": 1,
                "truncated": false
            }"#,
        )
        .unwrap();
        assert_eq!(collection.items.len(), 1);
        assert_eq!(collection.items[0].id, "PROJ-1");
        assert!(!collection.truncated);
    }
}
