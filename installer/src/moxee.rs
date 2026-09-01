use anyhow::Result;

use crate::MoxeeArgs;

pub async fn install(args: MoxeeArgs) -> Result<()> {
    let data_dir = args.data_dir.or(Some("/cache/rayhunter-data".to_string()));
    let persist_adb = args.persist_adb;
    let admin_ip = args.admin_ip.clone();
    let admin_username = args.admin_username.clone();
    let admin_password = args.admin_password.clone();

    // Done first, deliberately. The install reboots the device when it
    // finishes, and this only takes effect on a boot, so doing it afterwards
    // would race a device that is already going down. Writing it first means
    // the install's own reboot is the one that brings ADB up.
    if persist_adb {
        println!("Making ADB persist first, so the install's reboot brings it up...");
        crate::persist_adb::moxee(&admin_ip, &admin_username, admin_password.as_deref(), false)
            .await?;
        println!();
    }

    crate::orbic_network::install(
        args.admin_ip,
        args.admin_username,
        args.admin_password,
        args.reset_config,
        data_dir,
        args.enable_terminal,
    )
    .await?;

    if persist_adb {
        println!();
        println!("ADB will be enabled after this reboot, so later installs can use:");
        println!("  ./installer moxee-adb");
    }
    Ok(())
}
