// Integration tests for extract functionality
// Tests actual bbox extraction with fixtures

use std::io::Cursor;

use crate::extract::{BoundingBox, Extractor};
use crate::header::HEADER_SIZE;
use crate::{AsyncPmTilesReader, MmapBackend};

#[cfg(feature = "http-async")]
use crate::HttpBackend;

#[tokio::test]
async fn test_extract_firenze_small_bbox() {
    // Port of: TestExtract (go-pmtiles/pmtiles/extract_test.go:80)
    // Extract a small bbox from the Firenze fixture

    // Open the source file
    let backend = MmapBackend::try_from(crate::tests::VECTOR_FILE)
        .await
        .unwrap();
    let mut reader = AsyncPmTilesReader::try_from_source(backend).await.unwrap();

    // Small bbox in the center of Florence
    let bbox = BoundingBox::from_nesw(43.78, 11.26, 43.77, 11.24);

    // Extract to memory
    let mut output = Cursor::new(Vec::new());
    let extractor = Extractor::new(&mut reader);
    let stats = extractor
        .extract_bbox_to_writer(bbox, &mut output)
        .await
        .unwrap();

    // Verify we got some tiles
    assert_eq!(stats.addressed_tiles(), 31);
    assert_eq!(stats.tile_data_length(), 1_469_320);

    // Verify the output is a valid PMTiles archive
    let output_bytes = output.into_inner();
    assert!(
        output_bytes.len() >= HEADER_SIZE,
        "Output should have header"
    );
    assert_eq!(&output_bytes[0..7], b"PMTiles", "Should have magic bytes");

    // Write to temp file to test reading it back
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().join("extracted.pmtiles");
    std::fs::write(&temp_path, &output_bytes).unwrap();

    // Try to read the extracted archive
    let extracted_backend = MmapBackend::try_from(&temp_path).await.unwrap();
    let extracted_reader = AsyncPmTilesReader::try_from_source(extracted_backend)
        .await
        .unwrap();

    // Verify header properties
    let header = extracted_reader.get_header();
    assert!(header.clustered, "Extracted archive should be clustered");
    assert_eq!(
        stats.addressed_tiles(),
        header.n_tile_entries.unwrap().get(),
        "Plan entries should match header"
    );
}

#[tokio::test]
async fn test_extract_with_zoom_range() {
    // Test extracting with specific zoom range
    let backend = MmapBackend::try_from(crate::tests::VECTOR_FILE)
        .await
        .unwrap();
    let mut reader = AsyncPmTilesReader::try_from_source(backend).await.unwrap();

    // Bbox covering most of Florence
    let bbox = BoundingBox::from_nesw(43.83, 11.33, 43.73, 11.15);

    let mut output = Cursor::new(Vec::new());
    let extractor = Extractor::new(&mut reader).min_zoom(10).max_zoom(12);
    let stats = extractor
        .extract_bbox_to_writer(bbox, &mut output)
        .await
        .unwrap();

    // Verify we got tiles
    assert_eq!(stats.addressed_tiles(), 10);

    let output_bytes = output.into_inner();

    // Write to temp file to read back
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path().join("extracted.pmtiles");
    std::fs::write(&temp_path, &output_bytes).unwrap();

    let extracted_backend = MmapBackend::try_from(&temp_path).await.unwrap();
    let extracted_reader = AsyncPmTilesReader::try_from_source(extracted_backend)
        .await
        .unwrap();

    let header = extracted_reader.get_header();
    assert!(header.min_zoom >= 10, "Min zoom should be at least 10");
    assert!(header.max_zoom <= 12, "Max zoom should be at most 12");
}

#[tokio::test]
async fn test_extract_overfetch_reduces_requests() {
    // Test that higher overfetch reduces number of requests
    let backend = MmapBackend::try_from(crate::tests::VECTOR_FILE)
        .await
        .unwrap();
    let mut reader = AsyncPmTilesReader::try_from_source(backend).await.unwrap();

    let bbox = BoundingBox::from_nesw(43.80, 11.28, 43.75, 11.20);

    // Extract with low overfetch
    let mut output1 = Cursor::new(Vec::new());
    let extractor = Extractor::new(&mut reader);
    let stats_low = extractor
        .extract_bbox_to_writer(bbox, &mut output1)
        .await
        .unwrap();

    // Re-open the reader for second extraction
    let backend2 = MmapBackend::try_from(crate::tests::VECTOR_FILE)
        .await
        .unwrap();
    let mut reader2 = AsyncPmTilesReader::try_from_source(backend2).await.unwrap();

    // Extract with high overfetch
    let mut output2 = Cursor::new(Vec::new());
    let extractor2 = Extractor::new(&mut reader2);
    let stats_high = extractor2
        .extract_bbox_to_writer(bbox, &mut output2)
        .await
        .unwrap();

    // Higher overfetch should reduce requests (but may transfer more bytes)
    assert!(
        stats_high.num_tile_reqs() <= stats_low.num_tile_reqs(),
        "Higher overfetch should reduce requests: low={} high={}",
        stats_low.num_tile_reqs(),
        stats_high.num_tile_reqs()
    );

    // Both should extract same tiles
    assert_eq!(
        stats_low.addressed_tiles(),
        stats_high.addressed_tiles(),
        "Should extract same number of tiles"
    );
}

#[cfg(feature = "http-async")]
#[tokio::test]
#[ignore] // Requires local server at http://localhost:8001
async fn test_extract_seattle_from_http() {
    // Test extracting Seattle area from HTTP backend
    // This test requires a local server running at:
    // http://localhost:8001/pmtiles/maps-earth-planet-v1.250915.pmtiles
    //
    // To run this test:
    // cargo test --all-features test_extract_seattle_from_http -- --ignored

    let url = "http://localhost:8001/pmtiles/maps-earth-planet-v1.250915.pmtiles";
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .build()
        .unwrap();

    let backend = HttpBackend::try_from(client, url).unwrap();
    let mut reader = AsyncPmTilesReader::try_from_source(backend).await.unwrap();

    // Seattle bbox: west=-122.462, south=47.394, east=-122.005, north=47.831
    let bbox = BoundingBox::from_nesw(47.831, -122.005, 47.394, -122.462);

    // Extract to memory
    let mut output = Cursor::new(Vec::new());
    let extractor = Extractor::new(&mut reader);
    let stats = extractor
        .extract_bbox_to_writer(bbox, &mut output)
        .await
        .unwrap();

    let output_bytes = output.into_inner();

    // Compare against expected fixture
    let expected_bytes = std::fs::read("fixtures/seattle.pmtiles").unwrap();
    assert_eq!(
        output_bytes.len(),
        expected_bytes.len(),
        "Output size should match fixture"
    );
    assert_eq!(
        output_bytes, expected_bytes,
        "Extracted output should match fixture exactly"
    );

    // Verify stats for documentation
    println!("Successfully extracted {} tiles", stats.addressed_tiles());
    println!("Tile data length: {} bytes", stats.tile_data_length());
    println!(
        "Total bytes transferred: {} bytes",
        stats.total_tile_transfer_bytes()
    );
    println!("Output file size: {} bytes", output_bytes.len());
}
