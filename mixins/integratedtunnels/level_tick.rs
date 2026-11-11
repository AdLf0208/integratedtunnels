// After the level's tick: run every queued cable network's imports and exports
// (`native_level_tick_post`, API 23).
fn observe_level_tick_post(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::integratedtunnels_state::on_level_tick_post(game, dimension);
}
