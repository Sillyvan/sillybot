use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sysinfo::{
    DiskRefreshKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, MemoryRefreshKind, ProcessRefreshKind,
    ProcessesToUpdate, System, get_current_pid,
};

use crate::bot::{Context, Error};

/// Display this Sillybot instance's release and runtime information.
#[poise::command(slash_command, guild_only)]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let started = Instant::now();
    let mut resources = RuntimeResources::start()?;
    let response = ctx.say("Measuring command latency...").await?;
    let latency = started.elapsed();
    let resources = resources.measure(&ctx.data().database_path).await?;
    response
        .edit(
            ctx,
            poise::CreateReply::default().content(info_message(
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
                latency,
                resources,
            )),
        )
        .await?;
    Ok(())
}

struct RuntimeResources {
    system: System,
    pid: sysinfo::Pid,
    sample_started: Instant,
}

struct ResourceUsage {
    process_cpu_percent: f32,
    process_memory_mib: f64,
    system_cpu_percent: f32,
    system_used_memory_mib: f64,
    system_total_memory_mib: f64,
    storage_used_gib: f64,
    storage_total_gib: f64,
}

impl RuntimeResources {
    fn start() -> Result<Self, Error> {
        let mut system = System::new();
        let pid = get_current_pid()
            .map_err(|error| anyhow::anyhow!("failed to obtain current process PID: {error}"))?;
        refresh_process(&mut system, pid);
        refresh_system(&mut system);
        Ok(Self {
            system,
            pid,
            sample_started: Instant::now(),
        })
    }

    async fn measure(&mut self, database_path: &Path) -> Result<ResourceUsage, Error> {
        tokio::time::sleep(
            MINIMUM_CPU_UPDATE_INTERVAL.saturating_sub(self.sample_started.elapsed()),
        )
        .await;
        refresh_process(&mut self.system, self.pid);
        refresh_system(&mut self.system);
        let process = self
            .system
            .process(self.pid)
            .ok_or_else(|| anyhow::anyhow!("current process disappeared while handling /info"))?;
        let (storage_used_gib, storage_total_gib) = storage_usage(database_path)?;

        Ok(ResourceUsage {
            process_cpu_percent: process.cpu_usage(),
            process_memory_mib: bytes_to_mib(process.memory()),
            system_cpu_percent: self.system.global_cpu_usage(),
            system_used_memory_mib: bytes_to_mib(self.system.used_memory()),
            system_total_memory_mib: bytes_to_mib(self.system.total_memory()),
            storage_used_gib,
            storage_total_gib,
        })
    }
}

fn refresh_process(system: &mut System, pid: sysinfo::Pid) {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cpu().with_memory(),
    );
}

fn refresh_system(system: &mut System) {
    system.refresh_cpu_usage();
    system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn storage_usage(database_path: &Path) -> Result<(f64, f64), Error> {
    let database_path = absolute_path(database_path)?;
    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
    let disk = disks
        .list()
        .iter()
        .filter(|disk| database_path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "failed to find filesystem containing database path {}",
                database_path.display()
            )
        })?;
    let total = disk.total_space();
    let used = total.saturating_sub(disk.available_space());
    Ok((bytes_to_gib(used), bytes_to_gib(total)))
}

fn absolute_path(path: &Path) -> Result<PathBuf, Error> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn info_message(
    version: &str,
    operating_system: &str,
    architecture: &str,
    latency: Duration,
    resources: ResourceUsage,
) -> String {
    let ResourceUsage {
        process_cpu_percent,
        process_memory_mib,
        system_cpu_percent,
        system_used_memory_mib,
        system_total_memory_mib,
        storage_used_gib,
        storage_total_gib,
    } = resources;
    format!(
        "Sillybot v{version}\nRuntime: {operating_system}/{architecture}\nSillybot process: CPU {process_cpu_percent:.2}%, RAM {process_memory_mib:.2} MiB\nSystem: CPU {system_cpu_percent:.2}%, RAM {system_used_memory_mib:.2} / {system_total_memory_mib:.2} MiB, Storage {storage_used_gib:.2} / {storage_total_gib:.2} GiB\nCommand latency: {} ms",
        latency.as_millis()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_process_and_system_resource_usage() {
        assert_eq!(
            info_message(
                "1.2.3",
                "linux",
                "x86_64",
                Duration::from_millis(17),
                ResourceUsage {
                    process_cpu_percent: 4.25,
                    process_memory_mib: 48.75,
                    system_cpu_percent: 32.5,
                    system_used_memory_mib: 4096.0,
                    system_total_memory_mib: 8192.0,
                    storage_used_gib: 5.0,
                    storage_total_gib: 15.0,
                },
            ),
            "Sillybot v1.2.3\nRuntime: linux/x86_64\nSillybot process: CPU 4.25%, RAM 48.75 MiB\nSystem: CPU 32.50%, RAM 4096.00 / 8192.00 MiB, Storage 5.00 / 15.00 GiB\nCommand latency: 17 ms"
        );
    }
}
