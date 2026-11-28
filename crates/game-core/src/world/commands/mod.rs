mod build;
mod tactical;

use crate::protocol::ClientMessage;
use crate::types::PlayerId;

use super::World;

impl World {
    pub fn apply_command(&mut self, player_id: PlayerId, cmd: ClientMessage) -> Option<u8> {
        if !self.players.contains_key(&player_id) {
            return Some(1);
        }

        match cmd {
            ClientMessage::Build {
                building_kind,
                rel_x,
                rel_y,
            } => self.cmd_build(player_id, building_kind, rel_x, rel_y),
            ClientMessage::UpgradeBuilding { building_id } => {
                self.cmd_upgrade_building(player_id, building_id)
            }
            ClientMessage::SellBuilding { building_id } => {
                self.cmd_sell_building(player_id, building_id)
            }
            ClientMessage::ArmyCommand {
                squad,
                mode,
                target_x,
                target_y,
            } => self.cmd_army_command(player_id, squad, mode, target_x, target_y),
            ClientMessage::ConfigureBuilding { building_id, meta } => {
                self.cmd_configure_building(player_id, building_id, meta)
            }
            ClientMessage::TeleportZone { target_x, target_y } => {
                self.cmd_teleport_zone(player_id, target_x, target_y)
            }
            ClientMessage::ToggleBaseRegen => self.cmd_toggle_base_regen(player_id),
            _ => None,
        }
    }
}
