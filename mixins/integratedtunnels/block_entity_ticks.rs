// The tunnel cable ticks on the server (`native_block_entity_ticks`, API 15).
fn observe_block_entity_ticks(block: &str, ticks: &mut bool) {
    crate::game::integratedtunnels_state::on_block_entity_ticks(block, ticks);
}
