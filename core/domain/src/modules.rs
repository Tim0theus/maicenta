/// Modules exposed by the workspace shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceModule {
    Mail,
    Calendar,
    Tasks,
    Contacts,
    Notes,
    Vault,
    Assistant,
    Extensions,
}

impl WorkspaceModule {
    /// Modules that are visible in the first desktop prototype.
    pub const PROTOTYPE_MODULES: [Self; 4] =
        [Self::Mail, Self::Calendar, Self::Tasks, Self::Contacts];
}

/// Local lifecycle state for an optional module.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ModuleState {
    #[default]
    Enabled,
    Disabled,
    Uninstalled,
}

#[cfg(test)]
mod tests {
    use super::WorkspaceModule;

    #[test]
    fn prototype_starts_with_four_modules() {
        assert_eq!(WorkspaceModule::PROTOTYPE_MODULES.len(), 4);
    }
}
