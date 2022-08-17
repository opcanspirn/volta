use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::metadata::GlobalYarnManifest;
use crate::fs::read_dir_eager;

/// The package manager used to install a given package
#[derive(
    Copy, Clone, serde::Serialize, serde::Deserialize, PartialOrd, Ord, PartialEq, Eq, Debug,
)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

impl PackageManager {
    /// Given the `package_root`, returns the directory where the source is stored for this
    /// package manager. This will include the top-level `node_modules`, where appropriate.
    pub fn source_dir(self, package_root: PathBuf) -> PathBuf {
        let mut path = self.source_root(package_root);
        path.push("node_modules");

        path
    }

    /// Given the `package_root`, returns the root of the source directory. This directory will
    /// contain the top-level `node-modules`
    #[cfg(unix)]
    pub fn source_root(self, package_root: PathBuf) -> PathBuf {
        // On Unix, the source is always within a `lib` subdirectory, with both npm and Yarn
        let mut path = package_root;
        path.push("lib");

        path
    }

    /// Given the `package_root`, returns the root of the source directory. This directory will
    /// contain the top-level `node-modules`
    #[cfg(windows)]
    pub fn source_root(self, package_root: PathBuf) -> PathBuf {
        match self {
            // On Windows, npm/pnpm puts the source node_modules directory in the root of the `prefix`
            PackageManager::Npm | PackageManager::Pnpm => package_root,
            // On Windows, we still tell yarn to use the `lib` subdirectory
            PackageManager::Yarn => {
                let mut path = package_root;
                path.push("lib");

                path
            }
        }
    }

    /// Given the `package_root`, returns the directory where binaries are stored for this package
    /// manager.
    #[cfg(unix)]
    pub fn binary_dir(self, package_root: PathBuf) -> PathBuf {
        // On Unix, the binaries are always within a `bin` subdirectory for both npm and Yarn
        let mut path = package_root;
        path.push("bin");

        path
    }

    /// Given the `package_root`, returns the directory where binaries are stored for this package
    /// manager.
    #[cfg(windows)]
    pub fn binary_dir(self, package_root: PathBuf) -> PathBuf {
        match self {
            // On Windows, npm leaves the binaries at the root of the `prefix` directory
            PackageManager::Npm => package_root,
            // On Windows, pnpm/Yarn still includes the `bin` subdirectory
            PackageManager::Pnpm | PackageManager::Yarn => {
                let mut path = package_root;
                path.push("bin");

                path
            }
        }
    }

    /// Modify a given `Command` to be set up for global installs, given the package root
    pub fn setup_global_command(self, command: &mut Command, package_root: PathBuf) {
        command.env("npm_config_prefix", &package_root);

        if let PackageManager::Yarn = self {
            command.env("npm_config_global_folder", self.source_root(package_root));
        }

        // FIXME: Find out if there is a good way to redirect pnpm global installs
        // by using environment variables or whatever.
        if let PackageManager::Pnpm = self {}
    }

    /// Determine the name of the package that was installed into the `package_root`
    ///
    /// If there are none or more than one package installed, then we return None
    pub(super) fn get_installed_package(self, package_root: PathBuf) -> Option<String> {
        match self {
            PackageManager::Npm => get_npm_package_name(self.source_dir(package_root)),
            PackageManager::Pnpm | PackageManager::Yarn => {
                get_pnpm_or_yarn_package_name(self.source_root(package_root))
            }
        }
    }
}

/// Determine the package name for an npm global install
///
/// npm doesn't hoist the packages inside of `node_modules`, so the only directory will be the
/// globally installed package.
fn get_npm_package_name(mut source_dir: PathBuf) -> Option<String> {
    let possible_name = get_single_directory_name(&source_dir)?;

    // If the directory starts with `@`, that represents a scoped package, so we need to step
    // a level deeper to determine the full package name (`@scope/package`)
    if possible_name.starts_with('@') {
        source_dir.push(&possible_name);
        let package = get_single_directory_name(&source_dir)?;
        Some(format!("{}/{}", possible_name, package))
    } else {
        Some(possible_name)
    }
}

/// Return the name of the single subdirectory (if any) to the given `parent_dir`
///
/// If there are more than one subdirectory, then this will return `None`
fn get_single_directory_name(parent_dir: &Path) -> Option<String> {
    let mut entries = read_dir_eager(parent_dir)
        .ok()?
        .filter_map(|(entry, metadata)| {
            // If the entry is a symlink, _both_ is_dir() _and_ is_file() will be false. We want to
            // include symlinks as well as directories in our search, since `npm link` uses
            // symlinks internally, so we only exclude files from this search
            if !metadata.is_file() {
                Some(entry)
            } else {
                None
            }
        });

    match (entries.next(), entries.next()) {
        (Some(entry), None) => entry.file_name().into_string().ok(),
        _ => None,
    }
}

/// Determine the package name for a pnpm or Yarn global install
///
/// pnpm/Yarn creates a `package.json` file with the globally installed package as a dependency
fn get_pnpm_or_yarn_package_name(source_root: PathBuf) -> Option<String> {
    let package_file = source_root.join("package.json");
    let file = File::open(package_file).ok()?;
    let manifest: GlobalYarnManifest = serde_json::de::from_reader(file).ok()?;
    let mut dependencies = manifest.dependencies.into_iter();

    match (dependencies.next(), dependencies.next()) {
        // If there is exactly one dependency, we return it
        (Some((key, _)), None) => Some(key),
        // Otherwise, we can't determine the package name
        _ => None,
    }
}
