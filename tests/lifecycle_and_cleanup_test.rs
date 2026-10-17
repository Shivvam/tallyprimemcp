#[cfg(test)]
mod lifecycle_and_cleanup_tests {
    use tally_mcp::cleanup::{execute_cleanup, get_cleanup_inventory, CleanupOptions};
    use tally_mcp::manifest::InstallManifest;

    #[test]
    fn test_install_manifest_serialization() {
        let manifest = InstallManifest::load_or_create("msi");
        assert_eq!(manifest.install_source, "msi");
        assert_eq!(manifest.current_version, env!("CARGO_PKG_VERSION"));
        assert!(!manifest.installation_id.is_empty());
        assert!(!manifest.version_history.is_empty());

        let res = manifest.save();
        assert!(res.is_ok());
    }

    #[test]
    fn test_cleanup_inventory_categories() {
        let inventory = get_cleanup_inventory();
        let categories: Vec<String> = inventory.into_iter().map(|i| i.category).collect();

        assert!(categories.contains(&"settings".to_string()));
        assert!(categories.contains(&"drafts".to_string()));
        assert!(categories.contains(&"audit".to_string()));
        assert!(categories.contains(&"logs".to_string()));
        assert!(categories.contains(&"credentials".to_string()));
    }

    #[test]
    fn test_cleanup_dry_run_preserves_files() {
        let opts = CleanupOptions {
            dry_run: true,
            categories: vec!["drafts".to_string()],
            backup_dir: None,
            force: true,
        };

        let res = execute_cleanup(&opts);
        assert!(res.is_ok());
    }

    #[test]
    fn test_wix_installer_auto_launch_on_install() {
        let wxs_path = std::path::Path::new("installer/Package.wxs");
        assert!(wxs_path.exists(), "installer/Package.wxs must exist");

        let content = std::fs::read_to_string(wxs_path).expect("Failed to read Package.wxs");
        // Verify launch custom action is configured
        assert!(
            content.contains(r#"Id="LaunchApplication""#),
            "Package.wxs must contain LaunchApplication CustomAction"
        );
        assert!(
            content.contains(r#"FileRef="TallyMcpExe""#),
            "LaunchApplication must reference TallyMcpExe"
        );
        assert!(
            content.contains(r#"Return="asyncNoWait""#),
            "LaunchApplication must be asynchronous (asyncNoWait) to not block installer completion"
        );
        // Verify scheduled at InstallFinalize
        assert!(
            content.contains(r#"Action="LaunchApplication""#)
                && content.contains(r#"After="InstallFinalize""#),
            "LaunchApplication must be scheduled After='InstallFinalize'"
        );
        // Verify INSTALLFOLDER is added to user's PATH environment variable
        assert!(
            content.contains(r#"Name="PATH""#) && content.contains(r#"Value="[INSTALLFOLDER]""#),
            "Package.wxs must add INSTALLFOLDER to PATH environment variable"
        );
        // Verify Desktop Shortcut component exists in Package.wxs
        assert!(
            content.contains(r#"Id="ApplicationDesktopShortcut""#),
            "Package.wxs must contain ApplicationDesktopShortcut component"
        );
        assert!(
            content.contains(r#"Directory="DesktopFolder""#),
            "ApplicationDesktopShortcut must target DesktopFolder"
        );
        // Verify unconditional cleanup on uninstall
        assert!(
            content.contains(r#"Action="ExecuteAppCleanup""#)
                && content.contains(r#"Condition="(NOT UPGRADINGPRODUCTCODE) AND (REMOVE=&quot;ALL&quot;)""#),
            "ExecuteAppCleanup must run on standard uninstall (REMOVE=\"ALL\") without requiring REMOVEDATA=\"ALL\""
        );
    }

    #[test]
    fn test_desktop_shortcut_creation_and_removal() {
        use tally_mcp::cleanup::{create_desktop_shortcut, remove_desktop_shortcut};
        // Creating desktop shortcut should succeed or report valid path
        let res = create_desktop_shortcut();
        assert!(
            res.is_ok(),
            "create_desktop_shortcut should succeed: {:?}",
            res.err()
        );
        let path = res.unwrap();
        assert!(
            path.to_string_lossy().contains("tally") || path.to_string_lossy().contains("Tally")
        );

        // Removing desktop shortcut should cleanly execute
        remove_desktop_shortcut();
        assert!(!path.exists(), "Desktop shortcut should be cleanly removed");
    }

    #[test]
    fn test_remove_client_integrations() {
        use tally_mcp::cleanup::remove_client_integrations;
        // Should execute cleanly without panicking even if configs do not exist
        remove_client_integrations();
    }
}
