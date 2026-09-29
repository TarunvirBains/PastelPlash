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

#[test]
fn oot_reloaded_signs_are_never_grouped_or_abstracted() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/oot-reloaded.toml");
    let pack = Config::load(None, None, Some(&file)).unwrap().pack;
    for path in [
        "alt/objects/object_spot01_matoya/gKakarikoBazaarSignTex",
        "alt/objects/gameplay_keep/gSignLetteringTex",
        "alt/objects/object_mag/gTitleTheLegendOfTextTex",
    ] {
        assert!(!pack.grouping_allowed(Path::new(path)), "{path} grouped");
        assert!(
            !pack.abstraction_allowed(Path::new(path)),
            "{path} abstracted"
        );
    }
    for path in [
        "alt/scenes/shared/spot04_scene/spot04_room_0Tex_016508",
        "alt/objects/gameplay_keep/gHylianShieldDesignTex",
    ] {
        assert!(pack.grouping_allowed(Path::new(path)), "{path} not grouped");
        assert!(
            pack.abstraction_allowed(Path::new(path)),
            "{path} not abstracted"
        );
    }
}

#[test]
fn oot_reloaded_adult_temple_of_time_is_nocturne_child_is_not() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/oot-reloaded.toml");
    let pack = Config::load(None, None, Some(&file)).unwrap().pack;
    let adult = "alt/scenes/shared/shrine_r_scene/shrine_r_room_0Background_007AF0";
    assert_eq!(pack.classify(Path::new(adult)), Category::Background);
    assert_eq!(pack.mood_for(Path::new(adult)).name, "nocturne");
    for child in [
        "alt/scenes/shared/shrine_scene/shrine_room_0Background_007AF0",
        "alt/scenes/shared/shrine_n_scene/shrine_n_room_0Background_007B10",
    ] {
        assert_eq!(
            pack.classify(Path::new(child)),
            Category::Background,
            "{child}"
        );
        assert!(pack.mood_for(Path::new(child)).is_base(), "{child}");
    }
}

#[test]
fn oot_reloaded_nocturne_areas_have_their_casts() {
    use pastelplash::mood::cast_hue;
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/oot-reloaded.toml");
    let pack = Config::load(None, None, Some(&file)).unwrap().pack;
    for (path, cast) in [
        (
            "alt/scenes/nonmq/HAKAdan_scene/HAKAdan_room_0Tex_000000",
            "midnight-purple",
        ),
        (
            "alt/scenes/shared/hakaana_scene/hakaana_room_0Tex_000000",
            "midnight-purple",
        ),
        (
            "alt/scenes/nonmq/ydan_scene/ydan_room_0Tex_000000",
            "blue-teal",
        ),
        (
            "alt/scenes/mq/Bmori1_scene/Bmori1_room_0Tex_000000",
            "blue-teal",
        ),
        (
            "alt/scenes/nonmq/ganontika_scene/ganontika_room_0Tex_000000",
            "indigo",
        ),
        ("alt/textures/vr_RUVR_static/gMarketRuinsBgTex", "indigo"),
    ] {
        let m = pack.mood_for(Path::new(path));
        assert_eq!(m.name, "nocturne", "{path}");
        assert_eq!(m.cast_hue, cast_hue(cast), "{path}");
    }
    assert!(
        pack.mood_for(Path::new("alt/scenes/shared/spot04_scene/x"))
            .is_base()
    );
}

#[test]
fn oot_reloaded_actor_drawn_water_is_water() {
    // Water planes drawn by actors are not seen by the fluid detector (world only): without a
    // pack-map rule they took the actor path, and the engine-tinted dark well water
    // (gBotwWater2Tex) was raised to the tint-safe actor gray, near white.
    use pastelplash::config::FluidRuleKind;
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs/oot-reloaded.toml");
    let pack = Config::load(None, None, Some(&file)).unwrap().pack;
    for path in [
        "alt/objects/object_hakach_objects/gBotwWater1Tex",
        "alt/objects/object_hakach_objects/gBotwWater2Tex",
        "alt/objects/object_mizu_objects/object_mizu_objectsTex_007520",
    ] {
        let kind = pack.fluid_rule_for(Path::new(path)).and_then(|r| r.kind);
        assert!(
            matches!(kind, Some(FluidRuleKind::Water)),
            "{path}: {kind:?}"
        );
    }
}
