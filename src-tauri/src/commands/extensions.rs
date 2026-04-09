use crate::commands::extension_platform::{
    read_extension_manifest, read_vsix_manifest, sanitize_ext_id, user_extensions_dir,
    ExtensionManifest,
};
use serde::Serialize;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use tauri::AppHandle;

#[derive(Debug, Serialize)]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: String,
}

#[tauri::command]
pub async fn install_extension(vsix_path: String) -> Result<InstalledExtension, String> {
    let vsix = Path::new(&vsix_path);
    if !vsix.exists() {
        return Err(format!("VSIX not found: {vsix_path}"));
    }

    let file = File::open(vsix).map_err(|e| format!("open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("bad vsix: {e}"))?;

    let manifest = read_vsix_manifest(&mut archive)?;

    let safe_id = sanitize_ext_id(&manifest.id)?;
    let ext_dir = user_extensions_dir().join(&safe_id);
    if ext_dir.exists() {
        fs::remove_dir_all(&ext_dir).map_err(|e| format!("cleanup: {e}"))?;
    }
    fs::create_dir_all(&ext_dir).map_err(|e| format!("mkdir: {e}"))?;

    let prefix = "extension/";
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("entry: {e}"))?;
        let raw_name = entry.name().to_string();

        if !raw_name.starts_with(prefix) {
            continue;
        }

        let rel = &raw_name[prefix.len()..];
        if rel.is_empty() || rel.contains("..") {
            continue;
        }

        let target = ext_dir.join(rel);

        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("mkdir {rel}: {e}"))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).ok();
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|e| format!("read {rel}: {e}"))?;
            fs::write(&target, &buf).map_err(|e| format!("write {rel}: {e}"))?;
        }
    }

    log::info!("installed extension {} to {}", safe_id, ext_dir.display());

    Ok(InstalledExtension {
        id: safe_id,
        name: manifest.name,
        version: manifest.version,
        path: ext_dir.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn uninstall_extension(extension_id: String) -> Result<(), String> {
    let safe_id = sanitize_ext_id(&extension_id)?;
    let ext_dir = user_extensions_dir().join(&safe_id);
    if !ext_dir.exists() {
        return Err(format!("not installed: {extension_id}"));
    }
    fs::remove_dir_all(&ext_dir).map_err(|e| format!("remove: {e}"))?;
    log::info!("uninstalled {extension_id}");
    Ok(())
}

#[tauri::command]
pub async fn list_installed_extensions(app: AppHandle) -> Result<Vec<InstalledExtension>, String> {
    let dir = user_extensions_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| format!("readdir: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(ExtensionManifest {
            id,
            display_name,
            version,
            path,
            ..
        }) = read_extension_manifest(&app, &path)
        {
            out.push(InstalledExtension {
                id,
                name: display_name,
                version,
                path,
            });
        }
    }
    Ok(out)
}
