mod commands;
mod config;
mod download;
mod modules;
mod tui;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => commands::tui_cmd().await,
        Some("--version" | "-V") => {
            println!("dl {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h") => {
            print_help();
            Ok(())
        }
        Some(command) => {
            eprintln!(
                "[!] Comando desconocido '{command}'. dl-tui solo abre el dashboard — ejecuta 'dl' sin argumentos."
            );
            std::process::exit(1);
        }
    };

    if let Err(error) = result {
        eprintln!("[!] {error:#}");
        std::process::exit(1);
    }
}

fn print_help() {
    println!(
        "dl-tui v{} — dashboard no oficial de DockerLabs (https://dockerlabs.es)",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("  dl              abre el dashboard interactivo");
    println!("  dl --version    muestra la versión");
    println!("  dl --help       muestra esta ayuda");
    println!();
    println!("Todo vive dentro del dashboard: catálogo de máquinas, descargas,");
    println!("writeups, valoraciones, rankings, certificados y tu progreso.");
}
