// A placed tunnel cable declares a block entity for its network's item store
// (`native_block_entity`, API 15).
fn observe_block_entity(block: &str, entity: &mut Option<(String, horizon_world::nbt::Compound)>) {
    crate::game::integratedtunnels_state::on_block_entity(block, entity);
}
