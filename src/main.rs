use std::{collections::HashSet, time::Duration};

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_cloudwatchlogs::{Client, Error, Region};
use console::Style;
use futures::{stream::FuturesUnordered, StreamExt};
use gumdrop::Options;
use humantime::parse_duration;
use time::{format_description, OffsetDateTime};

#[derive(Debug, Options)]
struct CloudWatcherOptions {
    #[options(help = "print help message")]
    help: bool,
    #[options(help = "override region")]
    region: Option<String>,
    #[options(command)]
    command: Option<CloudWatcherCommands>,
}

#[derive(Debug, Options, PartialEq)]
enum CloudWatcherCommands {
    #[options(help = "list cloudwatch log groups")]
    List(CloudWatcherListOptions),
    #[options(help = "watch logs from cloudwatch log groups")]
    Watch(CloudWatcherWatchOptions),
}

#[derive(Debug, Options, PartialEq)]
struct CloudWatcherListOptions {
    #[options(help = "print help message")]
    help: bool,
}

#[derive(Debug, Options, PartialEq)]
struct CloudWatcherWatchOptions {
    #[options(help = "print help message")]
    help: bool,
    #[options(free, help = "cloudwatch groups to watch")]
    groups: Vec<String>,
    #[options(help = "refresh interval (default: 10s)")]
    refresh: Option<String>,
}

async fn list_log_groups(client: &Client) -> Result<(), Error> {
    let res = client.describe_log_groups().send().await?;
    let groups = res.log_groups.unwrap_or_default();

    for group in &groups {
        println!("{}", group.log_group_name().unwrap_or_default());
    }

    println!("Found {} log groups", groups.len());
    Ok(())
}

struct LogEvent {
    event_id: String,
    group: String,
    timestamp: OffsetDateTime,
    message: String,
}

async fn get_group_events(
    client: &Client,
    group: &str,
    start_time: i64,
) -> Result<Vec<LogEvent>, Error> {
    let res = client
        .filter_log_events()
        .log_group_name(group)
        .limit(100)
        .start_time(start_time)
        .send()
        .await?;

    Ok(res
        .events
        .unwrap_or_default()
        .into_iter()
        .map(|event| LogEvent {
            event_id: event.event_id.unwrap_or_default(),
            group: group.to_string(),
            timestamp: OffsetDateTime::from_unix_timestamp_nanos(
                event.timestamp.unwrap_or_default() as i128 * 1_000_000,
            )
            .expect("Failed to parse timestamp"),
            message: event
                .message
                .map(|msg| msg.trim().to_string())
                .unwrap_or_default(),
        })
        .collect())
}

async fn watch_log_groups(
    client: &Client,
    group_names: Vec<String>,
    refresh: Duration,
) -> Result<(), Error> {
    let format = format_description::parse(
        "[year]-[month]-[day] [hour]:[minute]:[second]:[subsecond digits:6]",
    )
    .unwrap();
    let mut seen_events: HashSet<String> = HashSet::new();

    let def = Style::new();
    let red = Style::new().red();
    let green = Style::new().green();
    let blue = Style::new().blue();
    let magenta = Style::new().magenta();
    let yellow = Style::new().yellow();

    loop {
        let start_time =
            (OffsetDateTime::now_utc() - Duration::from_secs(600)).unix_timestamp() * 1000;
        let queries = FuturesUnordered::new();
        for group in &group_names {
            queries.push(get_group_events(client, &group, start_time));
        }

        let results = queries.collect::<Vec<_>>().await;
        let mut new_events = Vec::new();

        for result in results {
            for event in result.unwrap_or_default() {
                if seen_events.insert(event.event_id.to_string()) {
                    new_events.push(event);
                }
            }
        }

        new_events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        for event in new_events {
            let timestamp = event.timestamp.format(&format).unwrap();
            let message = if event.message.contains("INFO") {
                blue.apply_to(event.message)
            } else if event.message.contains("ERROR") {
                red.apply_to(event.message)
            } else if event.message.contains("WARN") {
                yellow.apply_to(event.message)
            } else {
                def.apply_to(event.message)
            };

            println!(
                "{} {}: {}",
                green.apply_to(timestamp),
                magenta.apply_to(event.group),
                message
            )
        }

        tokio::time::sleep(refresh).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Gracefully terminate when we receive Ctrl+c
    ctrlc::set_handler(move || std::process::exit(0)).expect("Could not set Ctrl+c handler");

    // Parse the command-line arguments
    let options: CloudWatcherOptions = CloudWatcherOptions::parse_args_default_or_exit();

    // Figure out our region
    let region_provider = RegionProviderChain::first_try(options.region.map(Region::new))
        .or_default_provider()
        .or_else(Region::new("eu-west-1"));

    // Set up our logger
    env_logger::init();

    // Establish our AWS configuration and create the CloudWatch client
    let config = aws_config::from_env().region(region_provider).load().await;
    let client = Client::new(&config);

    // Parse the commands
    if let Some(command) = options.command {
        match command {
            CloudWatcherCommands::List(_) => list_log_groups(&client).await,
            CloudWatcherCommands::Watch(opts) => {
                let CloudWatcherWatchOptions {
                    groups, refresh, ..
                } = opts;

                if groups.is_empty() {
                    println!("No log groups to watch");
                    return Ok(());
                }

                watch_log_groups(
                    &client,
                    groups,
                    refresh
                        .map(|d| parse_duration(&d).expect("Failed to parse refresh duration"))
                        .unwrap_or_else(|| Duration::new(10, 0)),
                )
                .await?;
                Ok(())
            }
        }
    } else {
        println!("No command given");
        Ok(())
    }
}
