# moo-transport Examples

This directory contains examples demonstrating how to use the moo-transport library.

## roon_register.rs

Demonstrates connecting to a Roon Core via WebSocket and registering an extension.

### Usage

```bash
cargo run --example roon_register -- <ip> <port>
```

### Example

```bash
cargo run --example roon_register -- 192.168.1.100 9100
```

This example:
1. Connects to a Roon Core at the specified IP and port via WebSocket
2. Sends a registration request to `com.roonlabs.registry:1/register`
3. Displays the response messages from the Roon Core
4. Waits for the COMPLETE message and then closes the connection

### What it does

The example sends a MOO REQUEST message with a JSON body containing extension information:

```json
{
    "extension_id": "com.roonlabs.rust-moo-test",
    "display_name": "rust moo transport test",
    "display_version": "1.0.0",
    "publisher": "Danny",
    "email": "danny@roonlabs.com",
    "required_services": [],
    "optional_services": [],
    "provided_services": []
}
```

The Roon Core will respond with one or more messages (CONTINUE and/or COMPLETE) indicating the registration status.
