// One cable tick: queue the cable for the level tick's end
// (`native_block_entity_tick`, API 15).
fn observe_block_entity_tick(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    entity: &horizon_client::containers::ModBlockEntity,
) {
    crate::game::integratedtunnels_state::on_block_entity_tick(game, pos, entity);
}
