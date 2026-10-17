# Test data

- `tiny.jpg`: a 4 × 3 solid red (255, 0, 0) baseline JPEG, quality 95,
  written by the `image` crate 0.25.10's `JpegEncoder`
  (`RgbImage::from_pixel(4, 3, Rgb([255, 0, 0]))`).
- `gradient.jpg`: a 64 × 48 gradient (`Rgb([x * 4, y * 5, x * y % 256])`),
  quality 75, written the same way: its scan data is long enough to cut
  inside it.
- `truncated.jpg`: the first 40 bytes of `tiny.jpg` (its headers through
  the frame header, no scan data).

`image_io`'s tests include both (`decodes_png_and_jpeg`,
`a_hostile_image_is_refused_before_decoding`).
