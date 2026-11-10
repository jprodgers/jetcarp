//! The world a server acts on. The CLI uses a headless [`Session`]; the app implements [`Host`]
//! over its own state so tool calls go through its `exec(Command)` path.

use jetcarp_core::world::Applied;
use jetcarp_core::{Boundary, Command, Scene, TilingSpec, World};
use serde_json::{Value, json};

pub trait Host {
    /// The current world (read only).
    fn world(&self) -> &World;
    /// Apply one command. This is the only way tools change an existing world.
    fn exec(&mut self, cmd: Command) -> Result<Applied, String>;
    /// Replace the whole world with a scene (new world, open scene).
    fn replace(&mut self, scene: Scene) -> Result<(), String>;
    /// The current scene, as it would be saved.
    fn scene(&self) -> Scene {
        self.world().to_scene()
    }
    /// Facts about the host shown in `world_status` (e.g. whether the app is playing).
    fn info(&self) -> Value {
        json!({"kind": "headless"})
    }
    /// The app's displays and screensaver (the `app_display` tool, `app_tools.rs`).
    fn display(&mut self, _args: &Value) -> Result<Value, String> {
        Err(crate::app_tools::APP_ONLY.into())
    }
    /// Whether this host has [`display`](Self::display) (the app tools are listed only then).
    fn has_display(&self) -> bool {
        false
    }
}

/// A headless world for `jetcarp mcp`.
pub struct Session {
    pub world: World,
    pub name: String,
    pub description: String,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    /// An empty 64 x 64 wrapped square grid with no CAs.
    pub fn new() -> Session {
        Session {
            world: World::new(TilingSpec::rect("square", 64, 64, Boundary::Wrap), 1)
                .expect("default world"),
            name: String::new(),
            description: String::new(),
        }
    }
}

impl Host for Session {
    fn world(&self) -> &World {
        &self.world
    }
    fn exec(&mut self, cmd: Command) -> Result<Applied, String> {
        self.world.apply(cmd)
    }
    fn replace(&mut self, scene: Scene) -> Result<(), String> {
        self.world = World::from_scene(&scene)?;
        self.name = scene.name;
        self.description = scene.description;
        Ok(())
    }
    fn scene(&self) -> Scene {
        let mut s = self.world.to_scene();
        s.name = self.name.clone();
        s.description = self.description.clone();
        s
    }
    fn info(&self) -> Value {
        json!({"kind": "headless", "note": "a private world inside this MCP server; the JETCARP app is not affected"})
    }
}
