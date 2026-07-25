//! The CLI half of the machine-readable contract (see `mcp_server::contract`).
//!
//! Consumers verify "does this build still accept `analyze --format jsonl`" by grepping
//! `--help`, which couples their CI to the wording of help text rather than to the surface
//! itself. Everything here is read back out of the live clap definition, so a renamed flag
//! or a dropped enum value changes the declaration and a reworded help line does not.
//!
//! Scope: mutual exclusions are declared (`conflicts_with`), *requirements between* flags
//! are not — clap exposes the first through `Command::get_arg_conflicts_with` and offers
//! no getter for the second, and hand-copying `requires` would reintroduce exactly the
//! drift this module removes. So `--changed-files` needing `--incremental` is absent here.
//! A consumer must read the absence of a constraint as "not declared", never as "any
//! combination works".

use std::collections::{BTreeMap, BTreeSet};

use clap::builder::PossibleValue;
use clap::{Arg, Command};
use serde_json::{json, Map, Value};

/// Introspect a clap command tree into the contract's `cli` section.
///
/// The command is built first: an unbuilt `Command` has not yet inferred value counts, so
/// a repeatable flag would be declared as taking a single value.
pub fn cli_surface(root: &Command) -> Value {
    let mut built = root.clone();
    built.build();
    command_surface(&built, true)
}

fn command_surface(cmd: &Command, is_root: bool) -> Value {
    let mut entry = Map::new();
    entry.insert("name".into(), json!(cmd.get_name()));
    let aliases: Vec<&str> = cmd.get_all_aliases().collect();
    if !aliases.is_empty() {
        entry.insert("aliases".into(), json!(aliases));
    }
    if cmd.is_hide_set() {
        entry.insert("hidden".into(), json!(true));
    }

    // Global args are accepted by every subcommand, and building the tree copies them into
    // each one. Declaring them once at the root — flagged `global` — says the same thing
    // without repeating three entries under every command.
    let mut args: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|arg| !is_generated(arg) && (is_root || !arg.is_global_set()))
        .collect();
    args.sort_by_key(|arg| arg_name(arg).to_string());
    let conflicts = conflict_map(cmd, &args);
    entry.insert(
        "args".into(),
        Value::Array(
            args.into_iter().map(|arg| arg_surface(arg, conflicts.get(arg_name(arg)))).collect(),
        ),
    );

    // Subcommand order follows the declaration, matching the order `--help` prints them:
    // it is the order a reader compares against, and reordering it is not a contract change.
    let subcommands: Vec<Value> = cmd
        .get_subcommands()
        .filter(|sub| sub.get_name() != "help")
        .map(|sub| command_surface(sub, false))
        .collect();
    if !subcommands.is_empty() {
        entry.insert("commands".into(), Value::Array(subcommands));
    }
    Value::Object(entry)
}

/// Which flags cannot be combined, by flag name.
///
/// clap answers `get_arg_conflicts_with` from the asking argument's own blacklist, so a
/// conflict declared on one side is reported only from that side — while the parser
/// rejects the pair whichever order it is written in. Both directions are recorded here so
/// the declaration matches what the parser does rather than where the attribute was
/// written.
fn conflict_map<'a>(cmd: &'a Command, args: &[&'a Arg]) -> BTreeMap<&'a str, BTreeSet<&'a str>> {
    let declared: BTreeSet<&str> = args.iter().map(|arg| arg_name(arg)).collect();
    let mut map: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for arg in args {
        let name = arg_name(arg);
        for other in cmd.get_arg_conflicts_with(arg) {
            let other = arg_name(other);
            if other == name || !declared.contains(other) {
                continue;
            }
            map.entry(name).or_default().insert(other);
            map.entry(other).or_default().insert(name);
        }
    }
    map
}

/// clap synthesises `--help`/`--version` for every command; they are the parser's own
/// surface, not this tool's, and declaring them would add noise to every command.
fn is_generated(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

/// A flag is identified by its long form; a positional has none and goes by its id, which
/// is what `--help` shows in the usage line.
fn arg_name(arg: &Arg) -> &str {
    arg.get_long().unwrap_or_else(|| arg.get_id().as_str())
}

fn arg_surface(arg: &Arg, conflicts: Option<&BTreeSet<&str>>) -> Value {
    let mut entry = Map::new();
    entry.insert("name".into(), json!(arg_name(arg)));
    if arg.is_positional() {
        entry.insert("positional".into(), json!(true));
        // Positionals are matched by order, and the argument list here is sorted by name,
        // so the index is the only way a consumer can reconstruct the accepted order.
        if let Some(index) = arg.get_index() {
            entry.insert("index".into(), json!(index));
        }
    }
    if let Some(short) = arg.get_short() {
        entry.insert("short".into(), json!(short.to_string()));
    }
    // Hidden aliases are accepted just like visible ones, so both belong in a declaration
    // of what the build takes — `--help` is where the visible/hidden split matters.
    if let Some(aliases) = arg.get_all_aliases() {
        if !aliases.is_empty() {
            entry.insert("aliases".into(), json!(aliases));
        }
    }
    if let Some(short_aliases) = arg.get_all_short_aliases() {
        if !short_aliases.is_empty() {
            let short_aliases: Vec<String> =
                short_aliases.into_iter().map(|c| c.to_string()).collect();
            entry.insert("short_aliases".into(), json!(short_aliases));
        }
    }
    entry.insert("required".into(), json!(arg.is_required_set()));
    if arg.is_global_set() {
        entry.insert("global".into(), json!(true));
    }

    if let Some(conflicts) = conflicts {
        entry.insert("conflicts_with".into(), json!(conflicts));
    }

    // Always `Some` here — `cli_surface` builds the tree, which is what infers value
    // counts — but the field must be present either way, so consumers can read it without
    // treating "absent" as a third state.
    let range = arg.get_num_args().unwrap_or_else(|| (0..=0).into());
    entry.insert("takes_value".into(), json!(range.takes_values()));

    // A flag accepts more than one occurrence by several routes — a multi-value range, an
    // appending list, a counted switch (`-vvv`) — and only the first shows up in
    // `num_args`, which counts values per occurrence. Consumers care that repetition is
    // accepted, not by which route.
    let repeatable = range.max_values() > 1
        || matches!(arg.get_action(), clap::ArgAction::Append | clap::ArgAction::Count);
    if repeatable {
        entry.insert("multiple".into(), json!(true));
    }
    if let Some(delimiter) = arg.get_value_delimiter() {
        entry.insert("value_delimiter".into(), json!(delimiter.to_string()));
    }

    // Value aliases are accepted spellings of the same variant, so they belong beside the
    // canonical name: a consumer checking "is `jsonl` still accepted" must not miss it
    // because the build spells the variant differently and takes `jsonl` as an alias.
    let possible = arg.get_possible_values();
    let values: Vec<&str> = possible.iter().flat_map(PossibleValue::get_name_and_aliases).collect();
    if !values.is_empty() {
        entry.insert("values".into(), json!(values));
    }

    let defaults: Vec<String> =
        arg.get_default_values().iter().map(|v| v.to_string_lossy().into_owned()).collect();
    if !defaults.is_empty() {
        entry.insert("default".into(), json!(defaults));
    }
    Value::Object(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, ValueEnum};

    #[derive(Parser)]
    #[command(name = "demo")]
    struct Demo {
        #[arg(
            short = 's',
            long = "source-dir",
            alias = "src",
            short_alias = 'd',
            default_value = "."
        )]
        source_dir: String,

        #[arg(long, value_enum, default_value_t = Fmt::Console)]
        format: Fmt,

        #[arg(long)]
        quiet: bool,

        #[arg(long, value_delimiter = ',')]
        codes: Vec<String>,

        #[arg(short = 'v', action = clap::ArgAction::Count)]
        verbose: u8,

        #[arg(long, conflicts_with = "quiet")]
        write: bool,

        first: String,

        second: Option<String>,
    }

    #[derive(Clone, ValueEnum)]
    enum Fmt {
        Console,
        #[value(alias = "ndjson")]
        Jsonl,
    }

    fn arg<'a>(surface: &'a Value, name: &str) -> &'a Value {
        surface["args"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["name"] == name)
            .unwrap_or_else(|| panic!("no arg '{name}' in {surface:#}"))
    }

    #[test]
    fn declares_long_names_shorts_aliases_and_defaults() {
        let surface = cli_surface(&Demo::command());
        let source_dir = arg(&surface, "source-dir");
        assert_eq!(source_dir["short"], json!("s"));
        assert_eq!(source_dir["aliases"], json!(["src"]));
        assert_eq!(source_dir["short_aliases"], json!(["d"]));
        assert_eq!(source_dir["takes_value"], json!(true));
        assert_eq!(source_dir["default"], json!(["."]));
    }

    /// The enum values are the part downstream CI actually asserts on ("does this build
    /// still emit jsonl"), so they must survive into the declaration — including aliases,
    /// which are accepted spellings and not decoration.
    #[test]
    fn declares_value_enum_variants_with_their_aliases() {
        let surface = cli_surface(&Demo::command());
        assert_eq!(arg(&surface, "format")["values"], json!(["console", "jsonl", "ndjson"]));
    }

    #[test]
    fn distinguishes_switches_from_value_taking_args() {
        let surface = cli_surface(&Demo::command());
        assert_eq!(arg(&surface, "quiet")["takes_value"], json!(false));
        assert_eq!(arg(&surface, "codes")["multiple"], json!(true));
    }

    /// A counted switch takes no value but does accept repetition, and `num_args` alone
    /// cannot tell the two apart.
    #[test]
    fn declares_counted_switches_as_repeatable() {
        let surface = cli_surface(&Demo::command());
        let verbose = arg(&surface, "verbose");
        assert_eq!(verbose["takes_value"], json!(false));
        assert_eq!(verbose["multiple"], json!(true));
    }

    /// Mutual exclusion is a real part of what the build accepts, and it is derivable —
    /// unlike `requires`, which clap does not expose and which this module leaves out.
    #[test]
    fn declares_mutual_exclusions() {
        let surface = cli_surface(&Demo::command());
        assert_eq!(arg(&surface, "write")["conflicts_with"], json!(["quiet"]));
        assert_eq!(arg(&surface, "quiet")["conflicts_with"], json!(["write"]));
    }

    /// Positionals are matched by order, which the name-sorted argument list destroys.
    #[test]
    fn declares_positional_order() {
        let surface = cli_surface(&Demo::command());
        assert_eq!(arg(&surface, "first")["positional"], json!(true));
        assert_eq!(arg(&surface, "first")["index"], json!(1));
        assert_eq!(arg(&surface, "second")["index"], json!(2));
    }

    #[test]
    fn omits_clap_generated_help_and_version() {
        let surface = cli_surface(&Demo::command());
        let names: Vec<&str> = surface["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"help"), "{names:?}");
        assert!(!names.contains(&"version"), "{names:?}");
    }
}
