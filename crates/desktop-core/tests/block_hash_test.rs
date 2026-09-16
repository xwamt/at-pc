use at_pc_desktop_core::{compute_block_hashes, compute_block_hashes_sized, dirty_block_ratio};

#[test]
fn test_block_hash_determinism_and_single_byte_mutation() {
    let width = 1920;
    let height = 1080;
    let mut buffer = vec![128u8; (width * height * 4) as usize];

    let baseline = compute_block_hashes(&buffer, width, height);
    let second = compute_block_hashes(&buffer, width, height);
    assert_eq!(baseline, second, "哈希计算必须具备确定性与幂等性");

    // 修改单像素的单通道字节（测试敏感性与双射覆盖）
    let mutate_idx = (width as usize * 4 * 100) + (200 * 4 + 1);
    buffer[mutate_idx] ^= 0x01;
    let mutated = compute_block_hashes(&buffer, width, height);
    assert_ne!(baseline, mutated, "单字节变更必须导致块哈希改变");

    // 计算受影响的块坐标: x=200 -> col=200/64=3, y=100 -> row=100/64=1
    let cols = width.div_ceil(64) as usize;
    let target_block_idx = cols + 3;

    // 验证仅目标块哈希改变，其余块哈希完全一致
    for (i, (&orig, &curr)) in baseline.iter().zip(mutated.iter()).enumerate() {
        if i == target_block_idx {
            assert_ne!(orig, curr, "目标块哈希必须发生改变");
        } else {
            assert_eq!(orig, curr, "非目标块哈希不得受到任何影响 (块 {i})");
        }
    }

    let ratio = dirty_block_ratio(&baseline, &mutated);
    let total_blocks = baseline.len() as f64;
    let expected_ratio = 1.0 / total_blocks;
    assert!(
        (ratio - expected_ratio).abs() < 1e-9,
        "仅应引起单块变化: 实际 ratio={ratio}, 预期={expected_ratio}"
    );
}

#[test]
fn test_sensitivity_across_all_pixel_channels() {
    let width = 128;
    let height = 128;
    let base_buffer = vec![64u8; (width * height * 4) as usize];
    let baseline = compute_block_hashes(&base_buffer, width, height);

    // 针对单个像素的 R, G, B, A 四个通道逐一微扰
    let pixel_offset = (32 * width as usize + 32) * 4;
    for channel in 0..4 {
        let mut mutated_buffer = base_buffer.clone();
        mutated_buffer[pixel_offset + channel] ^= 0x80;
        let mutated = compute_block_hashes(&mutated_buffer, width, height);

        assert_ne!(
            baseline[0], mutated[0],
            "通道 {channel} 的单字节改动必须被块哈希感知"
        );
        assert_eq!(dirty_block_ratio(&baseline, &mutated), 1.0 / 4.0);
    }
}

#[test]
fn test_boundary_tiles_with_unaligned_trailing_bytes() {
    // 宽 65 高 65 时，block_size=64 会产生:
    // col 0: 宽 64 (256 字节，8 字节对齐)
    // col 1: 宽 1  (4 字节，非 8 字节对齐，必须正确走到尾部不足 8 字节分支)
    let width = 65;
    let height = 65;
    let mut buffer = vec![0u8; (width * height * 4) as usize];

    let baseline = compute_block_hashes_sized(&buffer, width, height, 64);
    assert_eq!(baseline.len(), 4, "2x2 块布局");

    // 修改 col 1, row 0 边界块中的单字节 (x=64, y=10)
    let trailing_pixel_idx = (10 * width as usize + 64) * 4 + 2;
    buffer[trailing_pixel_idx] = 0x5a;
    let mutated = compute_block_hashes_sized(&buffer, width, height, 64);

    assert_eq!(baseline[0], mutated[0]); // (0,0) 未改变
    assert_ne!(baseline[1], mutated[1]); // (1,0) 发生改变 (尾部不完整字)
    assert_eq!(baseline[2], mutated[2]); // (0,1) 未改变
    assert_eq!(baseline[3], mutated[3]); // (1,1) 未改变

    assert_eq!(dirty_block_ratio(&baseline, &mutated), 0.25);
}

#[test]
fn test_dirty_block_ratio_properties() {
    let empty: Vec<u64> = vec![];
    let h1 = vec![1, 2, 3, 4];
    let h2 = vec![1, 2, 3, 4];
    let h3 = vec![1, 99, 3, 4];
    let h4 = vec![1, 99, 88, 4];
    let h_different_len = vec![1, 2, 3];

    assert_eq!(dirty_block_ratio(&empty, &h1), 1.0);
    assert_eq!(dirty_block_ratio(&h1, &empty), 1.0);
    assert_eq!(dirty_block_ratio(&h1, &h_different_len), 1.0);
    assert_eq!(dirty_block_ratio(&h1, &h2), 0.0);
    assert_eq!(dirty_block_ratio(&h1, &h3), 0.25);
    assert_eq!(dirty_block_ratio(&h1, &h4), 0.5);
}

fn legacy_byte_by_byte_hashes(
    raw_rgba: &[u8],
    width: u32,
    height: u32,
    block_size: u32,
) -> Vec<u64> {
    if width == 0 || height == 0 || block_size == 0 {
        return Vec::new();
    }
    let cols = width.div_ceil(block_size);
    let rows = height.div_ceil(block_size);
    let stride = (width as usize).saturating_mul(4);
    let mut hashes = Vec::with_capacity((cols * rows) as usize);
    for by in 0..rows {
        let y0 = by * block_size;
        let y1 = (y0 + block_size).min(height);
        for bx in 0..cols {
            let x0 = bx * block_size;
            let x1 = (x0 + block_size).min(width);
            let mut hash: u64 = 0xcbf29ce484222325;
            for y in y0..y1 {
                let row = (y as usize).saturating_mul(stride);
                let start = row + (x0 as usize) * 4;
                let end = row + (x1 as usize) * 4;
                if end > raw_rgba.len() {
                    break;
                }
                for &byte in &raw_rgba[start..end] {
                    hash = hash.wrapping_mul(0x100000001b3) ^ u64::from(byte);
                }
            }
            hashes.push(hash);
        }
    }
    hashes
}

#[test]
fn test_speedup_1080p_word_wise_vs_byte_by_byte() {
    let width = 1920;
    let height = 1080;
    let mut buffer = vec![0u8; (width * height * 4) as usize];
    for (i, byte) in buffer.iter_mut().enumerate() {
        *byte = ((i * 13) ^ (i >> 3)) as u8;
    }

    // Warm-up
    let _ = compute_block_hashes(&buffer, width, height);

    let iterations = 10;
    let start_new = std::time::Instant::now();
    for _ in 0..iterations {
        let _ = compute_block_hashes(&buffer, width, height);
    }
    let dur_new = start_new.elapsed() / iterations;

    let start_old = std::time::Instant::now();
    for _ in 0..iterations {
        let _ = legacy_byte_by_byte_hashes(&buffer, width, height, 64);
    }
    let dur_old = start_old.elapsed() / iterations;

    let speedup = dur_old.as_secs_f64() / dur_new.as_secs_f64();
    println!(
        "1080p Buffer Benchmark (iterations={}): Old FNV-1a = {:?}, New Word-Wise = {:?}, Speedup = {:.2}x",
        iterations, dur_old, dur_new, speedup
    );

    if !cfg!(debug_assertions) {
        assert!(
            dur_new.as_micros() < 1500,
            "Release mode 1080p block hash should take <1.5ms, took {:?}",
            dur_new
        );
        assert!(
            speedup >= 8.0,
            "Expected >=8x-10x speedup in release mode, got {:.2}x",
            speedup
        );
    }
}
