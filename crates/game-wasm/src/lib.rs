use game_core::config::BalanceConfig;
use game_core::world::World;

const EMBEDDED_BALANCE: &str = include_str!("../../../config/balance.json");

pub struct WasmWorld {
    inner: World,
}

impl WasmWorld {
    pub fn new(seed: u64) -> Result<Self, String> {
        let config = BalanceConfig::from_json(EMBEDDED_BALANCE).map_err(|e| e.to_string())?;
        Ok(Self {
            inner: World::new(config, seed),
        })
    }

    pub fn step(&mut self, dt: f32) {
        self.inner.step(dt);
    }

    pub fn tick(&self) -> u32 {
        self.inner.tick
    }
}
