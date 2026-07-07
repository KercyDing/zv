const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe_mod = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
    });
    const exe = b.addExecutable(.{
        .name = "app",
        .root_module = exe_mod,
    });
    b.installArtifact(exe);

    const run_step = b.step("run", "Run the app");

    const run_cmd = b.addRunArtifact(exe);
    run_step.dependOn(&run_cmd.step);
    run_cmd.step.dependOn(b.getInstallStep());

    // Zig <= 0.17.0-dev.387+31f157d80
    if (b.args) |args| {
        run_cmd.addArgs(args);
    }

    // Zig > 0.17.0-dev.387+31f157d80
    // See PR for details: https://codeberg.org/ziglang/zig/pulls/35428
    // Uncomment below:
    //
    // run_cmd.addPassthruArgs();

    run_step.dependOn(&run_cmd.step);
}
