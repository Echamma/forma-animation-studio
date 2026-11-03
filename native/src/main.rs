fn main() -> eframe::Result {
    // An assistant's MCP client starts Forma this way: the stdio server that drives an open editor, not an editor.
    if std::env::args().any(|arg| arg == "--mcp-server") {
        std::process::exit(forma_native::mcp::serve());
    }
    canopy_ui::run_standalone(&forma_native::DESCRIPTOR)
}
