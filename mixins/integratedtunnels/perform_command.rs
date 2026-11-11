// A typed `/integratedtunnels` command reaches the native command hook
// (`native_perform_command`, API 8).
fn observe_perform_command(
    game: &mut crate::game::Game<'_>,
    player: &NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::integratedtunnels_state::on_perform_command(game, player, command, ran);
}
