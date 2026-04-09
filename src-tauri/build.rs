use std::fs;
use std::path::Path;

fn main() {
    let scripts_src = Path::new("../src/vs/workbench/contrib/terminal/common/scripts");
    let scripts_dst = Path::new("shell-integration");

    if scripts_src.exists() {
        fs::create_dir_all(scripts_dst).expect("Failed to create shell-integration dir");

        let shell_files = [
            "shellIntegration-rc.zsh",
            "shellIntegration-env.zsh",
            "shellIntegration-profile.zsh",
            "shellIntegration-login.zsh",
            "shellIntegration-bash.sh",
            "shellIntegration.fish",
            "shellIntegration.ps1",
        ];

        for file in &shell_files {
            let src = scripts_src.join(file);
            let dst = scripts_dst.join(file);
            if src.exists() {
                let src_bytes =
                    fs::read(&src).unwrap_or_else(|e| panic!("Failed to read {}: {}", file, e));
                let needs_copy = fs::read(&dst)
                    .map(|dst_bytes| dst_bytes != src_bytes)
                    .unwrap_or(true);
                if needs_copy {
                    fs::write(&dst, &src_bytes)
                        .unwrap_or_else(|e| panic!("Failed to copy {}: {}", file, e));
                }
                println!("cargo:rerun-if-changed={}", src.display());
            }
        }
    }

    tauri_build::build()
}
