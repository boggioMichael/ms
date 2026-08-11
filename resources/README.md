# Resources

Place the MapleStory test frame image here as `maplestory_hp_frame.png`.

The integration test `tests/hp_bar_integration.rs` loads this image and verifies
that the debug subsystem can detect the HP bar bounding rectangle. When the
screenshot is present, the test also saves an annotated overlay to `debug_out/`
for diagnostic inspection.
