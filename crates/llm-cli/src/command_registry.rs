//! Dynamic command registry for plugin command dispatch.
//!
//! This module implements the `CommandRegistry` as specified in ADR-001,
//! allowing plugin commands to be registered alongside core Clap commands.
//!
//! ## Collision Rules (from ADR-001)
//!
//! 1. **Core vs Plugin**: Core command wins; warning emitted to stderr.
//! 2. **Plugin vs Plugin**: First registered wins; warning emitted naming both plugins.
//! 3. Collisions are logged but do not cause failure (deterministic, non-breaking).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;

/// Handler function type for plugin commands.
///
/// Receives the command arguments (excluding the program name and command name)
/// and returns a Result indicating success or failure.
pub type CommandHandler = Arc<dyn Fn(&[String]) -> Result<()> + Send + Sync>;

/// Metadata and handler for a plugin-provided command.
#[derive(Clone)]
pub struct PluginCommand {
    /// The command name (e.g., "cluster", "jq").
    pub name: String,
    /// Human-readable description for help text.
    pub description: String,
    /// The plugin that registered this command.
    pub plugin_name: String,
    /// Handler function that executes the command.
    pub handler: CommandHandler,
}

impl std::fmt::Debug for PluginCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginCommand")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("plugin_name", &self.plugin_name)
            .field("handler", &"<fn>")
            .finish()
    }
}

impl PluginCommand {
    /// Create a new plugin command.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        plugin_name: impl Into<String>,
        handler: CommandHandler,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            plugin_name: plugin_name.into(),
            handler,
        }
    }

    /// Execute the command with the given arguments.
    pub fn execute(&self, args: &[String]) -> Result<()> {
        (self.handler)(args)
    }
}

/// Registry for core and plugin commands.
///
/// Implements the command resolution and collision handling specified in ADR-001.
#[derive(Debug, Default)]
pub struct CommandRegistry {
    /// Core command names (compiled-in, always available).
    core_commands: HashSet<String>,
    /// Plugin-provided commands (discovered at runtime).
    plugin_commands: HashMap<String, PluginCommand>,
    /// Collision warnings collected during registration.
    collision_warnings: Vec<String>,
}

impl CommandRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry pre-populated with core command names.
    pub fn with_core_commands(commands: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let core_commands = commands.into_iter().map(Into::into).collect();
        Self {
            core_commands,
            plugin_commands: HashMap::new(),
            collision_warnings: Vec::new(),
        }
    }

    /// Register a core command name.
    ///
    /// Core commands take precedence over plugin commands.
    pub fn register_core(&mut self, name: impl Into<String>) {
        self.core_commands.insert(name.into());
    }

    /// Register a plugin command.
    ///
    /// Returns `true` if the command was registered successfully.
    /// Returns `false` if it collided with a core or existing plugin command.
    ///
    /// ## Collision Rules
    ///
    /// - If the name matches a core command, the plugin command is skipped
    ///   and a warning is emitted.
    /// - If the name matches an existing plugin command, the new command is
    ///   skipped and a warning is emitted naming both plugins.
    pub fn register_plugin(&mut self, command: PluginCommand) -> bool {
        let name = &command.name;

        // Rule 1: Core vs plugin - core wins
        if self.core_commands.contains(name) {
            let warning = format!(
                "warning: plugin '{}' attempted to register command '{}' which is a core command; skipped",
                command.plugin_name, name
            );
            eprintln!("{}", warning);
            self.collision_warnings.push(warning);
            return false;
        }

        // Rule 2: Plugin vs plugin - first registered wins
        if let Some(existing) = self.plugin_commands.get(name) {
            let warning = format!(
                "warning: plugin '{}' attempted to register command '{}' which is already registered by plugin '{}'; skipped",
                command.plugin_name, name, existing.plugin_name
            );
            eprintln!("{}", warning);
            self.collision_warnings.push(warning);
            return false;
        }

        // No collision, register the command
        self.plugin_commands.insert(name.clone(), command);
        true
    }

    /// Check if a command name is a core command.
    pub fn is_core_command(&self, name: &str) -> bool {
        self.core_commands.contains(name)
    }

    /// Check if a command name is a registered plugin command.
    pub fn is_plugin_command(&self, name: &str) -> bool {
        self.plugin_commands.contains_key(name)
    }

    /// Get a plugin command by name.
    pub fn get_plugin_command(&self, name: &str) -> Option<&PluginCommand> {
        self.plugin_commands.get(name)
    }

    /// Get all registered plugin commands.
    pub fn plugin_commands(&self) -> impl Iterator<Item = &PluginCommand> {
        self.plugin_commands.values()
    }

    /// Get all core command names.
    pub fn core_commands(&self) -> impl Iterator<Item = &String> {
        self.core_commands.iter()
    }

    /// Get collision warnings that occurred during registration.
    pub fn collision_warnings(&self) -> &[String] {
        &self.collision_warnings
    }

    /// Clear collision warnings.
    pub fn clear_warnings(&mut self) {
        self.collision_warnings.clear();
    }

    /// Get the number of registered plugin commands.
    pub fn plugin_command_count(&self) -> usize {
        self.plugin_commands.len()
    }

    /// Get the number of core commands.
    pub fn core_command_count(&self) -> usize {
        self.core_commands.len()
    }

    /// Dispatch a command by name.
    ///
    /// Returns `Some(Result)` if a plugin command was found and executed,
    /// or `None` if the command should be handled by core dispatch.
    pub fn dispatch(&self, name: &str, args: &[String]) -> Option<Result<()>> {
        // Core commands take precedence - return None to let Clap handle it
        if self.is_core_command(name) {
            return None;
        }

        // Try plugin command dispatch
        self.plugin_commands.get(name).map(|cmd| cmd.execute(args))
    }

    /// Generate help text for plugin commands.
    ///
    /// Returns a formatted string suitable for appending to the main help output.
    pub fn plugin_commands_help(&self) -> String {
        if self.plugin_commands.is_empty() {
            return String::new();
        }

        let mut lines = vec!["\nPlugin Commands:".to_string()];

        // Sort commands alphabetically for consistent output
        let mut commands: Vec<_> = self.plugin_commands.values().collect();
        commands.sort_by(|a, b| a.name.cmp(&b.name));

        // Find the longest command name for alignment
        let max_name_len = commands.iter().map(|c| c.name.len()).max().unwrap_or(0);

        for cmd in commands {
            lines.push(format!(
                "  {:width$}  {}",
                cmd.name,
                cmd.description,
                width = max_name_len
            ));
        }

        lines.join("\n")
    }

    /// Print help for a specific plugin command.
    pub fn print_plugin_command_help(&self, name: &str) {
        if let Some(cmd) = self.plugin_commands.get(name) {
            println!("{}", cmd.name);
            println!();
            println!("{}", cmd.description);
            println!();
            println!("Provided by plugin: {}", cmd.plugin_name);
        }
    }
}

/// Get the list of core command names from the CLI.
///
/// This function returns the names of all commands defined in the Clap Command enum.
/// These names are used to detect collisions with plugin commands.
pub fn get_core_command_names() -> HashSet<String> {
    // These are the subcommand names from the Command enum in main.rs
    [
        "prompt",
        "plugins",
        "models",
        "keys",
        "aliases",
        "logs",
        "templates",
        "schemas",
        "tools",
        "embed",
        "embed-models",
        "embed-multi",
        "similar",
        "collections",
        "chat",
        "cmd",
        "version",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Create a command registry initialized with core commands.
pub fn create_registry() -> CommandRegistry {
    CommandRegistry::with_core_commands(get_core_command_names())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handler(_args: &[String]) -> Result<()> {
        Ok(())
    }

    fn make_test_command(name: &str, plugin: &str) -> PluginCommand {
        PluginCommand::new(
            name,
            format!("Test command from {}", plugin),
            plugin,
            Arc::new(test_handler),
        )
    }

    #[test]
    fn test_core_command_registration() {
        let mut registry = CommandRegistry::new();
        registry.register_core("logs");
        registry.register_core("models");

        assert!(registry.is_core_command("logs"));
        assert!(registry.is_core_command("models"));
        assert!(!registry.is_core_command("unknown"));
        assert_eq!(registry.core_command_count(), 2);
    }

    #[test]
    fn test_plugin_command_registration() {
        let mut registry = CommandRegistry::new();
        let cmd = make_test_command("cluster", "llm-cluster");

        assert!(registry.register_plugin(cmd));
        assert!(registry.is_plugin_command("cluster"));
        assert_eq!(registry.plugin_command_count(), 1);
    }

    #[test]
    fn test_core_vs_plugin_collision() {
        let mut registry = CommandRegistry::new();
        registry.register_core("logs");

        let cmd = make_test_command("logs", "evil-plugin");
        assert!(!registry.register_plugin(cmd));

        // Plugin command should not be registered
        assert!(!registry.is_plugin_command("logs"));
        assert!(registry.is_core_command("logs"));

        // Warning should be recorded
        assert_eq!(registry.collision_warnings().len(), 1);
        assert!(registry.collision_warnings()[0].contains("core command"));
    }

    #[test]
    fn test_plugin_vs_plugin_collision() {
        let mut registry = CommandRegistry::new();

        let cmd1 = make_test_command("cluster", "llm-cluster");
        let cmd2 = make_test_command("cluster", "another-plugin");

        assert!(registry.register_plugin(cmd1));
        assert!(!registry.register_plugin(cmd2));

        // First plugin should own the command
        let registered = registry.get_plugin_command("cluster").unwrap();
        assert_eq!(registered.plugin_name, "llm-cluster");

        // Warning should be recorded
        assert_eq!(registry.collision_warnings().len(), 1);
        assert!(registry.collision_warnings()[0].contains("already registered by plugin"));
    }

    #[test]
    fn test_dispatch_core_command() {
        let mut registry = CommandRegistry::new();
        registry.register_core("logs");

        // Core command dispatch should return None (let Clap handle it)
        let result = registry.dispatch("logs", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_plugin_command() {
        let mut registry = CommandRegistry::new();

        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        let cmd = PluginCommand::new(
            "custom",
            "A custom command",
            "test-plugin",
            Arc::new(move |_args| {
                called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
        );

        registry.register_plugin(cmd);

        let result = registry.dispatch("custom", &[]);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_dispatch_unknown_command() {
        let registry = CommandRegistry::new();

        // Unknown command should return None
        let result = registry.dispatch("unknown", &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_plugin_commands_help() {
        let mut registry = CommandRegistry::new();

        registry.register_plugin(make_test_command("zebra", "plugin-z"));
        registry.register_plugin(make_test_command("alpha", "plugin-a"));

        let help = registry.plugin_commands_help();
        assert!(help.contains("Plugin Commands:"));
        assert!(help.contains("alpha"));
        assert!(help.contains("zebra"));

        // Should be sorted alphabetically
        let alpha_pos = help.find("alpha").unwrap();
        let zebra_pos = help.find("zebra").unwrap();
        assert!(alpha_pos < zebra_pos);
    }

    #[test]
    fn test_empty_plugin_commands_help() {
        let registry = CommandRegistry::new();
        let help = registry.plugin_commands_help();
        assert!(help.is_empty());
    }

    #[test]
    fn test_with_core_commands() {
        let registry = CommandRegistry::with_core_commands(["logs", "models", "keys"]);

        assert!(registry.is_core_command("logs"));
        assert!(registry.is_core_command("models"));
        assert!(registry.is_core_command("keys"));
        assert_eq!(registry.core_command_count(), 3);
    }

    #[test]
    fn test_get_core_command_names() {
        let names = get_core_command_names();

        // Check that expected core commands are present
        assert!(names.contains("prompt"));
        assert!(names.contains("plugins"));
        assert!(names.contains("models"));
        assert!(names.contains("keys"));
        assert!(names.contains("logs"));
        assert!(names.contains("chat"));
        assert!(names.contains("cmd"));
        assert!(names.contains("version"));
    }

    #[test]
    fn test_create_registry() {
        let registry = create_registry();

        // Should have core commands pre-registered
        assert!(registry.is_core_command("logs"));
        assert!(registry.is_core_command("models"));
        assert!(!registry.is_plugin_command("logs"));
    }

    #[test]
    fn test_command_handler_receives_args() {
        let mut registry = CommandRegistry::new();

        let received_args = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_args_clone = received_args.clone();

        let cmd = PluginCommand::new(
            "echo",
            "Echo arguments",
            "test-plugin",
            Arc::new(move |args| {
                *received_args_clone.lock().unwrap() = args.to_vec();
                Ok(())
            }),
        );

        registry.register_plugin(cmd);

        let test_args = vec!["arg1".to_string(), "arg2".to_string(), "--flag".to_string()];
        registry.dispatch("echo", &test_args);

        let captured = received_args.lock().unwrap();
        assert_eq!(*captured, test_args);
    }

    #[test]
    fn test_plugin_command_iterator() {
        let mut registry = CommandRegistry::new();

        registry.register_plugin(make_test_command("cmd1", "plugin1"));
        registry.register_plugin(make_test_command("cmd2", "plugin2"));

        let names: Vec<_> = registry.plugin_commands().map(|c| c.name.clone()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"cmd1".to_string()));
        assert!(names.contains(&"cmd2".to_string()));
    }

    #[test]
    fn test_clear_warnings() {
        let mut registry = CommandRegistry::new();
        registry.register_core("logs");

        let cmd = make_test_command("logs", "evil-plugin");
        registry.register_plugin(cmd);

        assert_eq!(registry.collision_warnings().len(), 1);

        registry.clear_warnings();
        assert!(registry.collision_warnings().is_empty());
    }
}
