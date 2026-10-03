//! Geometry is Syrup's: rectangles, pixel-run segmentation, region
//! grouping, colour bars and their fill, uniform panels, text blocks.

pub use syrup::color::{is_color_pixel, is_text_pixel};
pub use syrup::geometry::{
    Rect, dominant_color_bucket, find_color_bar, find_color_regions, find_text_block,
    find_text_block_in_regions, find_uniform_color_panel, group_segments, measure_bar_fill,
    segment_row,
};
