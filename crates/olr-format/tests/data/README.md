# Test data

- `tiny.jpg`: a 4 × 3 solid red (255, 0, 0) baseline JPEG, quality 95,
  written by the `image` crate 0.25.10's `JpegEncoder`
  (`RgbImage::from_pixel(4, 3, Rgb([255, 0, 0]))`).
- `truncated.jpg`: the first 40 bytes of `tiny.jpg` (its headers through
  the frame header, no scan data).

`image_io`'s tests include both (`decodes_png_and_jpeg`,
`a_hostile_image_is_refused_before_decoding`).
