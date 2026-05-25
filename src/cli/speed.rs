use crate::{
    App, Result, ZvError,
    app::{
        CacheStrategy,
        network::mirror::{MirrorBenchmarkResult, MirrorManager, RankApplyPolicy},
        utils::{host_target, zig_tarball},
    },
};
use color_eyre::eyre::eyre;
use dialoguer::{Select, theme::ColorfulTheme};
use yansi::Paint;

const DEFAULT_SAMPLE_SIZE_MIB: u64 = 4;
const DEFAULT_CONCURRENCY: usize = 4;

pub async fn speed(
    mut app: App,
    refresh: bool,
    concurrency: usize,
    sample_size: u64,
    json: bool,
) -> Result<()> {
    let cache_strategy = if refresh {
        CacheStrategy::AlwaysRefresh
    } else {
        CacheStrategy::PreferCache
    };

    tokio::fs::create_dir_all(&app.paths.cache_dir)
        .await
        .map_err(ZvError::Io)?;
    let mut mirror_manager =
        MirrorManager::init_and_load(app.paths.mirrors_file.clone(), cache_strategy)
            .await
            .map_err(ZvError::NetworkError)?;

    let release = app.fetch_latest_version(cache_strategy).await?;
    let semver_version = release.resolved_version().version().clone();
    let host_target = host_target().ok_or_else(|| eyre!("Could not determine host target"))?;
    let artifact = release.target_artifact(&host_target).ok_or_else(|| {
        eyre!(
            "No download artifact found for target <{}> in release {}",
            host_target,
            release.version_string()
        )
    })?;
    let zig_tarball = zig_tarball(&semver_version, None).ok_or_else(|| {
        eyre!(
            "Could not determine tarball name for Zig version {}",
            semver_version
        )
    })?;
    let requested_sample_size = sample_size.max(1).saturating_mul(1024 * 1024);
    let sample_size = requested_sample_size.min(artifact.size);
    let concurrency = concurrency.max(1);

    if !json {
        println!(
            "{} {} using up to {:.1} MiB per mirror",
            Paint::new("Benchmarking mirrors for").italic(),
            Paint::cyan(&zig_tarball),
            sample_size as f64 / 1_048_576.0
        );
    }

    let results = mirror_manager
        .benchmark_mirrors(&semver_version, &zig_tarball, sample_size, concurrency)
        .await
        .map_err(ZvError::NetworkError)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    print_results(&results);

    let Some(policy) = prompt_application_policy()? else {
        println!("{}", Paint::yellow("No changes saved.").italic());
        return Ok(());
    };

    let applied_label = match policy {
        RankApplyPolicy::Overwrite => "overwritten",
        RankApplyPolicy::Blend => "blended",
    };

    mirror_manager
        .apply_benchmark_results(&results, policy)
        .await
        .map_err(ZvError::NetworkError)?;

    println!(
        "{}",
        Paint::green(&format!("Mirror ranks {applied_label} and saved.")).bold()
    );

    Ok(())
}

pub fn default_sample_size() -> u64 {
    DEFAULT_SAMPLE_SIZE_MIB
}

pub fn default_concurrency() -> usize {
    DEFAULT_CONCURRENCY
}

fn print_results(results: &[MirrorBenchmarkResult]) {
    println!();
    println!("{}", "Mirror benchmark results:".italic());
    println!();

    for (idx, result) in results.iter().enumerate() {
        let rank = format!("#{}", idx + 1);
        let rank_display = if result.is_success() {
            Paint::green(&rank).bold().to_string()
        } else {
            Paint::red(&rank).to_string()
        };

        match result.bytes_per_second {
            Some(bytes_per_second) => {
                let mib_per_second = bytes_per_second / 1_048_576.0;
                let layout = result
                    .measured_layout
                    .map(|layout| format!("{layout:?}").to_lowercase())
                    .unwrap_or_else(|| "unknown".to_string());
                println!(
                    "  {} {:>8.2} MiB/s  old rank #{:<3} {} ({})",
                    rank_display,
                    mib_per_second,
                    result.old_rank,
                    result.base_url,
                    Paint::cyan(&layout).italic()
                );
            }
            None => {
                println!(
                    "  {} {:>8}        old rank #{:<3} {} ({})",
                    rank_display,
                    "failed",
                    result.old_rank,
                    result.base_url,
                    Paint::red(result.error.as_deref().unwrap_or("unknown error")).italic()
                );
            }
        }
    }

    println!();
}

fn prompt_application_policy() -> Result<Option<RankApplyPolicy>> {
    if !crate::tools::supports_interactive_prompts() {
        println!(
            "{}",
            Paint::yellow("Interactive prompt unavailable; leaving mirrors.toml unchanged.")
                .italic()
        );
        return Ok(None);
    }

    let options = [
        "Overwrite all ranks with benchmark order (default)",
        "Blend benchmark order with existing ranks",
        "Do nothing and exit",
    ];
    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Apply benchmark results?")
        .items(options)
        .default(0)
        .interact()
        .map_err(|e| ZvError::General(eyre!(e)))?;

    Ok(match selection {
        0 => Some(RankApplyPolicy::Overwrite),
        1 => Some(RankApplyPolicy::Blend),
        2 => None,
        _ => unreachable!("invalid dialoguer selection"),
    })
}
