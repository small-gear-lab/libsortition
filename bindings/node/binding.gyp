{
  # libsortition のパスは `node-gyp rebuild --sortition_root=/path/to/repo` で変えられる
  # （include/sortition.h と、target/release か lib/ にある libsortition.so を探す）
  "variables": {
    "sortition_root%": "<(module_root_dir)/../.."
  },
  "targets": [
    {
      "target_name": "sortition_node",
      "sources": ["src/sortition_node.c"],
      "include_dirs": ["<(sortition_root)/core/include"],
      "cflags": ["-std=c11", "-Wall", "-Wextra"],
      "conditions": [
        ["OS=='linux' or OS=='mac'", {
          "libraries": [
            "-L<(sortition_root)/target/release",
            "-lsortition",
            "-Wl,-rpath,<(sortition_root)/target/release"
          ]
        }]
      ]
    }
  ]
}
