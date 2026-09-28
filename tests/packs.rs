//! Pack maps in `packs/` load and classify representative paths as documented in PLAN.md.

use std::path::Path;

use pastelplash::config::{Category, Config};

fn classify(pack: &str, path: &str) -> Category {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("packs")
        .join(pack);
    let config = Config::load(None, None, Some(&file)).unwrap();
    config.pack.classify(Path::new(path))
}

#[test]
fn oot_reloaded_paths_map_to_categories() {
    let cases = [
        (
            "alt/objects/object_link_boy/gLinkAdultTunicTex",
            Category::Actor,
        ),
        (
            "alt/objects/object_link_boy/gLinkAdultEyesOpenTex",
            Category::Skip,
        ),
        (
            "alt/objects/object_link_boy/gLinkAdultMouth1Tex",
            Category::Skip,
        ),
        (
            "alt/objects/gameplay_field_keep/gFieldBushTex",
            Category::Actor,
        ),
        (
            "alt/objects/gameplay_field_keep/gFieldDoorTex",
            Category::World,
        ),
        (
            "alt/objects/object_wood02/object_wood02_Tex_000F90",
            Category::World,
        ),
        ("alt/objects/object_spot04_objects/tex", Category::World),
        (
            "alt/scenes/shared/spot04_scene/spot04_room_0Tex_014B08",
            Category::World,
        ),
        (
            "alt/textures/vr_fine0_static/gSunriseSkybox1Tex",
            Category::Skybox,
        ),
        ("alt/textures/parameter_static/gHeartFullTex", Category::Ui),
        ("alt/something/else", Category::Skip),
        // Pre-rendered scene images are backgrounds, not skies or tiling world textures.
        (
            "alt/textures/vr_RUVR_static/gMarketRuinsBgTex",
            Category::Background,
        ),
        (
            "alt/textures/vr_SP1a_static/gBazaarBgTex",
            Category::Background,
        ),
        (
            "alt/scenes/shared/shrine_r_scene/shrine_r_room_0Background_007AF0",
            Category::Background,
        ),
        (
            "alt/textures/vr_holy0_static/gHoly0Skybox1Tex",
            Category::Skybox,
        ),
        (
            "alt/textures/vr_cloud3_static/gNightOvercastSkybox1Tex",
            Category::Skybox,
        ),
    ];
    for (path, want) in cases {
        assert_eq!(classify("oot-reloaded.toml", path), want, "{path}");
    }
}
