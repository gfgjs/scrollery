// 构建与发货材料共用版本，防止 dispatcher 和许可证来自不同提交。
export const VPL_VERSION = '2.17.0';
export const VPL_COMMIT = 'd77f9195cf495b937631607333288fd917ae8939';
export const VPL_LICENSE_SOURCES = [
  {
    id: 'libvpl-license',
    kind: 'libvpl',
    target: 'libvpl/LICENSE',
    url: `https://raw.githubusercontent.com/intel/libvpl/${VPL_COMMIT}/LICENSE`,
    sha256: 'bf1cfac2e2792b6e1e995ce103d70796aecaf2ec7e4c5fe5474f7acec7b4a677',
  },
  {
    id: 'libvpl-third-party-programs',
    kind: 'libvpl',
    target: 'libvpl/third-party-programs.txt',
    url: `https://raw.githubusercontent.com/intel/libvpl/${VPL_COMMIT}/third-party-programs.txt`,
    sha256: 'e3c92992b1df02b467ac086d7d3730f4c1fc2c2a1f2c520657c4c9f2085305a3',
  },
];
