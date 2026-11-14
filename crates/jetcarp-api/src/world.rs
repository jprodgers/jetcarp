//! The world a front end acts on. A headless front end (`jetcarp mcp`, tests) keeps its own
//! [`Session`]; the app implements [`Host`] over the world the user is watching, so every change
//! goes through its `exec(Command)` path and shows up (and is logged) like a change made by hand.

use jetcarp_core::world::Applied;
use jetcarp_core::{Boundary, Command, Scene, TilingSpec, World};
use serde_json::{Value, json};

/// The error a host without a display gives for the app's display tool (`app_display`).
pub const APP_ONLY: &str = "app_display is only available in the JETCARP app: start the app with AI control on (or `jetcarp --control`), then use `jetcarp mcp --app` or call use_app with attach=true";

/// A world that can be read and changed through [`Command`]s.
pub trait Host {
    /// The current world (read only).
    fn world(&self) -> &World;
    /// Apply one command. This is the only way an existing world is changed, except for
    /// [`world_mut`](Self::world_mut).
    fn exec(&mut self, cmd: Command) -> Result<Applied, String>;
    /// The world itself, so `world.step` can step it on the GPU (with the same result and log as
    /// `exec`ing the steps). None: steps go through `exec` (the app steps its own world).
    fn world_mut(&mut self) -> Option<&mut World> {
        None
    }
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
    /// The app's displays and screensaver (the MCP `app_display` tool).
    fn display(&mut self, _args: &Value) -> Result<Value, String> {
        Err(APP_ONLY.into())
    }
    /// Whether this host has [`display`](Self::display) (the app tools are listed only then).
    fn has_display(&self) -> bool {
        false
    }
}

/// A headless world, with the name and description a saved scene carries.
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
    fn world_mut(&mut self) -> Option<&mut World> {
        Some(&mut self.world)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_replaces_and_names_its_world() {
        let mut s = Session::new();
        assert_eq!(s.world().topo.len(), 64 * 64);
        assert!(s.display(&json!({})).is_err());
        assert!(!s.has_display());
        let mut scene = s.scene();
        scene.name = "test".into();
        scene.description = "a scene".into();
        s.replace(scene).unwrap();
        assert_eq!(s.scene().name, "test");
        assert_eq!(s.scene().description, "a scene");
    }
}
