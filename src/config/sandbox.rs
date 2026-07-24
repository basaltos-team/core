// mlua sandbox setup and blocked global validation.

use std::fs;
use std::path::{Path, PathBuf};

use mlua::{Lua, Value};

pub fn new_sandboxed_lua(path: &Path) -> Result<Lua, String> {
    let lua = Lua::new();
    remove_unsafe_globals(&lua, path)?;
    Ok(lua)
}

pub fn new_sandboxed_lua_with_modules(path: &Path, module_root: &Path) -> Result<Lua, String> {
    let lua = Lua::new();
    remove_unsafe_globals(&lua, path)?;

    let module_root = module_root
        .canonicalize()
        .map_err(|err| format!("{}: {err}", module_root.display()))?;
    let require_root = module_root.clone();
    let require = lua
        .create_function(move |lua, module: String| {
            let module_path =
                resolve_module_path(&require_root, &module).map_err(mlua::Error::external)?;
            let source = fs::read_to_string(&module_path).map_err(|err| {
                mlua::Error::external(format!("{}: {err}", module_path.display()))
            })?;

            lua.load(&source)
                .set_name(module_path.display().to_string())
                .eval::<Value>()
        })
        .map_err(|err| {
            format!(
                "{}: failed to install sandboxed Lua require: {err}",
                path.display()
            )
        })?;

    let globals = lua.globals();
    globals.set("require", require).map_err(|err| {
        format!(
            "{}: failed to install sandboxed Lua require: {err}",
            path.display()
        )
    })?;

    Ok(lua)
}

fn remove_unsafe_globals(lua: &Lua, path: &Path) -> Result<(), String> {
    let globals = lua.globals();
    for name in [
        "os",
        "io",
        "require",
        "loadfile",
        "dofile",
        "package",
        "debug",
        "collectgarbage",
    ] {
        globals.set(name, Value::Nil).map_err(|err| {
            format!(
                "{}: failed to remove unsafe Lua global `{name}`: {err}",
                path.display()
            )
        })?;
    }

    Ok(())
}

fn resolve_module_path(root: &Path, module: &str) -> Result<PathBuf, String> {
    if module.is_empty() {
        return Err("empty module name".to_string());
    }

    let mut relative = PathBuf::new();
    for part in module.split('.') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains('/')
            || part.contains('\\')
        {
            return Err(format!("unsupported module name `{module}`"));
        }
        relative.push(part);
    }
    relative.set_extension("lua");

    let path = root.join(relative);
    let canonical = path
        .canonicalize()
        .map_err(|err| format!("{}: {err}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(format!("module `{module}` escapes config directory"));
    }

    Ok(canonical)
}
