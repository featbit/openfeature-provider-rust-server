use std::env;
use std::error::Error;
use std::fmt::Display;
use std::time::{Duration, Instant};

use featbit_openfeature_provider::FeatBitProvider;
use open_feature::{
    Client, EvaluationContext, EvaluationDetails, EvaluationError, EvaluationReason, OpenFeature,
    StructValue,
};

const DEFAULT_STREAMING_URL: &str = "wss://app-eval.featbit.co";
const DEFAULT_EVENT_URL: &str = "https://app-eval.featbit.co";
const DEFAULT_TARGETING_KEY: &str = "openfeature-example-user";
const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;
const MIN_POLL_INTERVAL_MS: u64 = 100;

type AnyError = Box<dyn Error + Send + Sync>;

#[derive(Clone, Copy)]
enum FlagType {
    Bool,
    Int,
    Float,
    String,
    Struct,
}

impl FlagType {
    fn parse(value: Option<String>) -> Result<Self, String> {
        match value.as_deref().unwrap_or("bool") {
            "bool" | "boolean" => Ok(Self::Bool),
            "int" | "integer" => Ok(Self::Int),
            "float" | "number" => Ok(Self::Float),
            "string" => Ok(Self::String),
            "struct" | "object" | "json" => Ok(Self::Struct),
            value => Err(format!(
                "unsupported flag type {value:?}; expected bool, int, float, string, or struct"
            )),
        }
    }
}

impl Display for FlagType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::String => "string",
            Self::Struct => "struct",
        };
        formatter.write_str(value)
    }
}

struct Arguments {
    flag_key: String,
    flag_type: FlagType,
    targeting_key: String,
    attributes: Vec<(String, String)>,
}

impl Arguments {
    fn parse() -> Result<Option<Self>, String> {
        let mut arguments = env::args().skip(1);
        let Some(flag_key) = arguments.next() else {
            return Ok(None);
        };
        if matches!(flag_key.as_str(), "-h" | "--help") {
            return Ok(None);
        }
        if flag_key.trim().is_empty() {
            return Err("flag key must not be empty".to_owned());
        }

        let flag_type = FlagType::parse(arguments.next())?;
        let targeting_key = arguments
            .next()
            .unwrap_or_else(|| DEFAULT_TARGETING_KEY.to_owned());
        if targeting_key.trim().is_empty() {
            return Err("targeting key must not be empty".to_owned());
        }

        let attributes = arguments
            .map(|argument| {
                let (name, value) = argument.split_once('=').ok_or_else(|| {
                    format!("invalid attribute {argument:?}; expected name=value")
                })?;
                if name.is_empty() {
                    return Err(format!(
                        "invalid attribute {argument:?}; attribute name must not be empty"
                    ));
                }
                Ok((name.to_owned(), value.to_owned()))
            })
            .collect::<Result<_, _>>()?;

        Ok(Some(Self {
            flag_key,
            flag_type,
            targeting_key,
            attributes,
        }))
    }

    fn context(&self) -> EvaluationContext {
        let mut context =
            EvaluationContext::default().with_targeting_key(self.targeting_key.clone());
        for (name, value) in &self.attributes {
            context.add_custom_field(name.clone(), value.clone());
        }
        context
    }
}

#[derive(Debug, PartialEq)]
enum EvaluatedValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Struct(StructValue),
}

#[derive(Debug, PartialEq)]
struct EvaluationSnapshot {
    value: EvaluatedValue,
    variant: Option<String>,
    reason: Option<EvaluationReason>,
}

#[derive(Debug, PartialEq)]
enum EvaluationState {
    Resolved(EvaluationSnapshot),
    Error {
        code: String,
        message: Option<String>,
    },
}

impl EvaluationState {
    fn from_result(result: Result<EvaluationSnapshot, EvaluationError>) -> Self {
        match result {
            Ok(snapshot) => Self::Resolved(snapshot),
            Err(error) => Self::Error {
                code: error.code.to_string(),
                message: error.message,
            },
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let Some(arguments) =
        Arguments::parse().map_err(|message| format!("{message}\n\n{}", usage()))?
    else {
        println!("{}", usage());
        return Ok(());
    };

    let server_key = required_environment("FEATBIT_SERVER_KEY")?;
    let streaming_url =
        env::var("FEATBIT_STREAMING_URL").unwrap_or_else(|_| DEFAULT_STREAMING_URL.to_owned());
    let event_url = env::var("FEATBIT_EVENT_URL").unwrap_or_else(|_| DEFAULT_EVENT_URL.to_owned());
    let poll_interval = poll_interval()?;

    // Provider creation performs a bounded wait for the initial flag snapshot.
    let provider = tokio::task::spawn_blocking(move || {
        FeatBitProvider::from_endpoints(server_key, streaming_url, event_url)
    })
    .await??;

    let (client, provider_name) = {
        let mut api = OpenFeature::singleton_mut().await;
        api.set_provider(provider).await;
        let provider_name = api.provider_metadata().await.name;
        (api.create_client(), provider_name)
    };

    println!("provider={provider_name}");
    println!("flag_key={}", arguments.flag_key);
    println!("flag_type={}", arguments.flag_type);
    println!("targeting_key={}", arguments.targeting_key);
    println!("poll_interval_ms={}", poll_interval.as_millis());

    let context = arguments.context();
    let result = watch(&client, &arguments, &context, poll_interval).await;

    drop(client);
    OpenFeature::singleton_mut().await.shutdown().await;

    result
}

async fn evaluate(
    client: &Client,
    arguments: &Arguments,
    context: &EvaluationContext,
) -> Result<EvaluationSnapshot, EvaluationError> {
    match arguments.flag_type {
        FlagType::Bool => client
            .get_bool_details(&arguments.flag_key, Some(context), None)
            .await
            .map(|details| snapshot(details, EvaluatedValue::Bool)),
        FlagType::Int => client
            .get_int_details(&arguments.flag_key, Some(context), None)
            .await
            .map(|details| snapshot(details, EvaluatedValue::Int)),
        FlagType::Float => client
            .get_float_details(&arguments.flag_key, Some(context), None)
            .await
            .map(|details| snapshot(details, EvaluatedValue::Float)),
        FlagType::String => client
            .get_string_details(&arguments.flag_key, Some(context), None)
            .await
            .map(|details| snapshot(details, EvaluatedValue::String)),
        FlagType::Struct => client
            .get_struct_details::<StructValue>(&arguments.flag_key, Some(context), None)
            .await
            .map(|details| snapshot(details, EvaluatedValue::Struct)),
    }
}

async fn watch(
    client: &Client,
    arguments: &Arguments,
    context: &EvaluationContext,
    poll_interval: Duration,
) -> Result<(), AnyError> {
    let mut previous = EvaluationState::from_result(evaluate(client, arguments, context).await);
    print_state("initial", None, &previous);
    println!("watching=true; press Ctrl+C to stop");

    let started = Instant::now();
    let mut ticker = tokio::time::interval(poll_interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;

    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            signal_result = &mut shutdown => {
                signal_result?;
                println!("watching=false");
                return Ok(());
            }
            _ = ticker.tick() => {
                let current =
                    EvaluationState::from_result(evaluate(client, arguments, context).await);
                if current != previous {
                    print_state("changed", Some(started.elapsed()), &current);
                    previous = current;
                }
            }
        }
    }
}

fn snapshot<T>(
    details: EvaluationDetails<T>,
    into_value: impl FnOnce(T) -> EvaluatedValue,
) -> EvaluationSnapshot {
    EvaluationSnapshot {
        value: into_value(details.value),
        variant: details.variant,
        reason: details.reason,
    }
}

fn print_state(kind: &str, elapsed: Option<Duration>, state: &EvaluationState) {
    println!();
    println!("evaluation={kind}");
    if let Some(elapsed) = elapsed {
        println!("elapsed_ms={}", elapsed.as_millis());
    }
    match state {
        EvaluationState::Resolved(snapshot) => {
            println!("status=resolved");
            print_value(&snapshot.value);
            println!(
                "variant={}",
                snapshot.variant.as_deref().unwrap_or("<none>")
            );
            println!(
                "reason={}",
                snapshot
                    .reason
                    .as_ref()
                    .map_or_else(|| "<none>".to_owned(), ToString::to_string)
            );
        }
        EvaluationState::Error { code, message } => {
            println!("status=error");
            println!("error_code={code}");
            println!("error_message={}", message.as_deref().unwrap_or("<none>"));
        }
    }
}

fn print_value(value: &EvaluatedValue) {
    match value {
        EvaluatedValue::Bool(value) => println!("value={value}"),
        EvaluatedValue::Int(value) => println!("value={value}"),
        EvaluatedValue::Float(value) => println!("value={value}"),
        EvaluatedValue::String(value) => println!("value={value:?}"),
        EvaluatedValue::Struct(value) => println!("value={value:?}"),
    }
}

fn required_environment(name: &str) -> Result<String, AnyError> {
    match env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(format!("set the {name} environment variable before running the example").into()),
    }
}

fn poll_interval() -> Result<Duration, AnyError> {
    let value = env::var("OPENFEATURE_POLL_INTERVAL_MS")
        .unwrap_or_else(|_| DEFAULT_POLL_INTERVAL_MS.to_string());
    let milliseconds = value.parse::<u64>().map_err(|_| {
        format!("OPENFEATURE_POLL_INTERVAL_MS must be an integer, received {value:?}")
    })?;
    if milliseconds < MIN_POLL_INTERVAL_MS {
        return Err(format!(
            "OPENFEATURE_POLL_INTERVAL_MS must be at least {MIN_POLL_INTERVAL_MS}"
        )
        .into());
    }
    Ok(Duration::from_millis(milliseconds))
}

fn usage() -> &'static str {
    "Usage:
  cargo run --manifest-path examples/openfeature-only/Cargo.toml -- \
<flag-key> [bool|int|float|string|struct] [targeting-key] [name=value ...]

Environment:
  FEATBIT_SERVER_KEY       required; never put a server key in source control
  FEATBIT_STREAMING_URL    optional; defaults to wss://app-eval.featbit.co
  FEATBIT_EVENT_URL        optional; defaults to https://app-eval.featbit.co
  OPENFEATURE_POLL_INTERVAL_MS
                           optional; defaults to 1000 (minimum 100)"
}
