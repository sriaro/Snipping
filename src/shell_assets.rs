pub const PACKAGE: &str = "Snipping.ShellExtension";

const BASE: [(&str, u32); 3] = [("StoreLogo.png", 50), ("Square44x44Logo.png", 44), ("Square150x150Logo.png", 150)];
const TARGET_SIZES: [u32; 14] = [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256];

pub fn specs() -> impl Iterator<Item = (String, u32)> {
    BASE.into_iter()
        .map(|(name, size)| (name.to_string(), size))
        .chain(TARGET_SIZES.into_iter().flat_map(|size| ["", "_altform-unplated", "_altform-lightunplated"].map(move |suffix| (format!("Square44x44Logo.targetsize-{size}{suffix}.png"), size))))
}
