//! Example: Connect to a Roon Core and register an extension
//!
//! Usage:
//!   cargo run --example roon_register -- <ip> <port>
//!
//! Example:
//!   cargo run --example roon_register -- 192.168.1.100 9330

use futures::StreamExt;
use moo_transport::{MooConnection, MooRequest, Result};
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing for logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <ip> <port>", args[0]);
        eprintln!("Example: {} 192.168.1.100 9330", args[0]);
        std::process::exit(1);
    }

    let ip = &args[1];
    let port = &args[2];
    let ws_url = format!("ws://{}:{}/api", ip, port);

    println!("Connecting to Roon Core at {}", ws_url);

    // Connect to Roon Core via WebSocket
    let conn = MooConnection::new_websocket(&ws_url).await?;
    println!("Connected!");

    conn.register_request_handler("com.roonlabs.ping:1/ping", |req: MooRequest| async move {
        println!("Received ping request, returning COMPLETE Success response.");
        req.send_complete("Success", None).await
    }).await;

    // Prepare registration body
    let registration_body = json!({
        "extension_id": "com.roonlabs.rust-moo-test",
        "display_name": "rust moo transport test",
        "display_version": "1.0.0",
        "publisher": "Danny",
        "email": "danny@roonlabs.com",
        "required_services": [],
        "optional_services": [],
        "provided_services": [ "com.roonlabs.ping:1" ]
    });

    println!("Sending registration request...");

    // Send registration request
    let mut responses = conn
        .send_request("com.roonlabs.registry:1/register", Some(registration_body))
        .await?;

    println!("\nWaiting for responses...");

    // Process responses
    while let Some(result) = responses.next().await {
        match result {
            Ok(msg) => {
                println!("\n=== Received Response ===");
                println!("Verb: {:?}", msg.verb);
                println!("Name: {}", msg.name);
                println!("Request-Id: {}", msg.request_id);

                if let Some(body_result) = msg.body_json() {
                    match body_result {
                        Ok(body) => {
                            println!("Body:\n{}", serde_json::to_string_pretty(body).unwrap());
                        }
                        Err(e) => {
                            eprintln!("Error parsing body: {}", e);
                        }
                    }
                }

                // Exit after receiving COMPLETE
                if matches!(msg.verb, moo_transport::MooVerb::Complete) {
                    println!("\n=== Registration Complete ===");
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
