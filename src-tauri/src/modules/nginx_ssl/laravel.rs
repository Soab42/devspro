//! Laravel-specific project validation. Deliberately not a generic "any PHP
//! project" check — Laravel's webroot is `public/`, not the project root, and that
//! convention is what the nginx render step depends on.

use std::path::Path;

pub fn is_laravel_project(path: &Path) -> bool {
    path.join("artisan").is_file() && path.join("public").join("index.php").is_file()
}

#[tauri::command]
pub fn nginx_ssl_validate_laravel_project(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err(format!("{path} is not a directory"));
    }
    if !is_laravel_project(p) {
        return Err(
            "this folder doesn't look like a Laravel project — expected an `artisan` file and `public/index.php`"
                .to_string(),
        );
    }
    Ok(())
}
