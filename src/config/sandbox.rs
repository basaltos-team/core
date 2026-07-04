// mlua sandbox setup and blocked global validation.

use std::path::Path;

use mlua::{Lua, Value};

pub fn new_sandboxed_lua(path: &Path) -> Result<Lua, String> {
    let lua = Lua::new();
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

    Ok(lua)
}
