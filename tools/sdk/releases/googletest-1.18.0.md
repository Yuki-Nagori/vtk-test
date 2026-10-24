vtk-test 受信 CI（生产管线自原 Panta 迁入，任务 041）为 macOS arm64、Linux x86_64 和 Windows x86_64 构建并自检
GoogleTest 静态 SDK。每个归档都包含上游 CMake 包、头文件、静态库、源码元数据
和 BSD-3-Clause 许可证。本仓库已将制品登记到 `native/cmake/sdk-provision.cmake`；
启用 `BUILD_TESTING=ON` 时，native 测试通过 `panta_require_sdk` 使用已验证的平台
SDK。GoogleTest 仅供测试目标使用，关闭测试时不会下载或查找该 SDK。

## 源码与构建

- 上游：<https://github.com/google/googletest>
- 版本：`v1.18.0`；固定 commit：`063de7e9578f82b369302001269680b4b1553359`
- 构建：Release、C++17、静态库；关闭 GoogleMock；Windows 使用共享 CRT。
- 自检：归档内的 `GTestConfig.cmake` 必须提供 `GTest::gtest` 和
  `GTest::gtest_main`；CI 在打包前编译、链接并运行测试程序。
- 许可证：BSD-3-Clause；每个归档都包含从上游源码复制的
  `share/licenses/GoogleTest/LICENSE`。
- macOS arm64 制品以 macOS 13.0 为最低部署版本；Qt 与其他运行时依赖也须满足此基线。

Release 使用单一 tag `sdk-googletest-1.18.0`。`sdk-googletest.yml` 重新生产时，只有所有平台矩阵任务通过后才会清除旧资产并上传完整的三平台制品集。每个归档都有对应的 `.sha256` 校验文件；归档 URL 与 SHA256 以 `native/cmake/sdk-provision.cmake` 为准，升级时重新校验实际制品、链接与测试。
