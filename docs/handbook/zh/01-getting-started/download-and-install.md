---
id: download-and-install
track: getting-started
trackOrder: 1
chapterOrder: 1
kicker: "01.1 / 快速上手"
title: "下载、安装与第一次启动"
subtitle: "选对安装包，完成系统放行，并确认 NexFilm 已经可以正常工作。"
readingTime: 6
inApp: true
sinceVersion: "v1.1.0"
aliases: ["quick-start"]
---

## 先选对安装包 {#choose-installer}

拿到 NexFilm 之后，最容易卡住的一步其实不是安装，而是下错文件。GitHub Releases 页面会同时列出安装包、源码压缩包和校验文件，第一次打开时很容易把它们混在一起。你只需要看文件名里是否带有 `setup.exe` 或 `.dmg`，不需要下载 `Source code (zip)`。

Windows 用户选择文件名中带 `x64-setup.exe` 的安装程序。macOS 用户需要先确认自己的 Mac 是 Apple Silicon 还是 Intel：点击屏幕左上角的苹果菜单，选择 **关于本机**，如果芯片一栏写着 M1、M2、M3、M4 或更新的 Apple 芯片，就下载文件名中带 `aarch64.dmg` 的版本；如果写着 Intel，则下载带 `x64.dmg` 的版本。下载错架构通常不会损坏电脑，但应用无法正常启动，重新下载对应版本即可。

<table class="doc-table">
    <thead>
        <tr><th>电脑类型</th><th>要下载的文件</th><th>怎么确认</th></tr>
    </thead>
    <tbody>
        <tr>
            <td><strong>Windows 10 / 11</strong></td>
            <td><code>NexFilm.Engine_{version}_x64-setup.exe</code></td>
            <td>文件名以 <code>setup.exe</code> 结尾。</td>
        </tr>
        <tr>
            <td><strong>Apple Silicon Mac</strong></td>
            <td><code>NexFilm.Engine_{version}_aarch64.dmg</code></td>
            <td>“关于本机”的芯片一栏为 Apple M 系列。</td>
        </tr>
        <tr>
            <td><strong>Intel Mac</strong></td>
            <td><code>NexFilm.Engine_{version}_x64.dmg</code></td>
            <td>“关于本机”的处理器一栏为 Intel。</td>
        </tr>
    </tbody>
</table>

<div class="doc-callout">
    <div class="doc-callout-title">只从官方 Releases 下载</div>
    <div class="doc-callout-body">请从项目仓库的 Releases 页面下载安装包。网盘转载、第三方下载站和文件名被改过的版本都不建议使用。安装包没有商业代码签名，因此 Windows 与 macOS 在第一次启动时都可能给出安全提示，这是未签名开源软件的常见情况。</div>
</div>

## 安装 NexFilm Engine {#install}

Windows 上的安装过程比较直接。双击下载好的 `.exe` 文件，如果 Windows Defender SmartScreen 弹出“Windows 已保护你的电脑”，点击弹窗中的 **更多信息**，再点击 **仍要运行**。随后按照安装向导的提示完成安装即可。NexFilm 默认会安装到系统程序目录，安装完成后可以在开始菜单中找到它。

macOS 用户双击 `.dmg` 文件，系统会打开一个挂载的磁盘映像窗口。把里面的 `NexFilm Engine.app` 拖到 **应用程序** 文件夹，然后在访达的应用程序列表中找到它。拖拽完成后可以先推出磁盘映像，安装包本身可以保留，也可以稍后删除。

第一次打开时，系统可能会提示应用“已损坏”，或者提示“Apple 无法检查其是否包含恶意软件”。这不代表文件真的损坏，通常只是 macOS 给下载文件加上的隔离属性。先在 **应用程序** 文件夹中确认 `NexFilm Engine.app` 已经存在，再继续下一节。

## 第一次启动与 macOS 放行 {#first-launch}

Windows 用户如果在启动安装程序时已经点击过 **仍要运行**，安装完成后一般可以直接打开 NexFilm。如果系统再次拦截，仍然从提示窗口进入 **更多信息**，选择继续运行即可。建议不要关闭 Windows Defender 或系统级安全功能，也没有必要为了运行 NexFilm 修改全局安全策略。

macOS 用户请打开 **启动台** → **其他** → **终端**，也可以从 **访达** → **应用程序** → **实用工具** 中打开终端。复制下面这一条完整命令，粘贴到终端窗口后按 <kbd>Return</kbd>：

```bash
xattr -cr "/Applications/NexFilm Engine.app"
```

如果系统要求输入密码，输入当前 Mac 的登录密码后再按一次 <kbd>Return</kbd>。终端输入密码时不会显示字符，看不到星号或圆点都是正常的。命令执行完成后，回到 **应用程序** 文件夹重新打开 `NexFilm Engine`。

<div class="doc-callout">
    <div class="doc-callout-title">这条命令只处理 NexFilm</div>
    <div class="doc-callout-body">`xattr -cr` 会清除指定应用的扩展属性，其中也包括下载隔离标记。它只作用于后面的路径，不会关闭 Gatekeeper，也不会改动系统完整性保护（SIP）。如果应用没有安装在 <code>/Applications</code>，需要把命令中的路径换成实际位置。</div>
</div>

## 第一次打开后先确认什么 {#first-open}

启动成功后，NexFilm 默认会进入 **图库**。第一次使用时图库可能是空的，这是正常现象，说明还没有导入任何扫描文件。顶栏右侧的 **导入胶卷** 是日常工作的起点；如果你只是拿到几张测试片，也可以先用散张导入快速看效果，等熟悉流程后再处理整卷。

这时不必急着导入几十张照片。更稳妥的做法是找两三张曝光正常的底片，先走一遍“导入 → 工作台 → 设置胶片范围 → 自动反相 → 导出”的完整流程。确认缩略图、画布预览和导出文件都能正常打开之后，再处理整卷素材。NexFilm 不会复制、移动或改写你的原始 RAW、TIFF 文件，项目数据库与缩略图缓存则保存在系统的应用支持目录中。

如果图库已经能看到缩略图，但双击进入工作台后画面没有立即变成正片，也不用担心。NexFilm 会在第一次打开某一帧时生成工作代理，并等待你执行 **自动反相**。这是正常的按需处理过程，不代表导入失败。

## 安装与启动常见问题 {#common-issues}

**下载下来的文件打不开。** 先确认下载的是安装包而不是 `Source code` 压缩包。Windows 安装程序应以 `.exe` 结尾，macOS 安装包应以 `.dmg` 结尾。若文件名与平台不匹配，重新回到 Releases 页面选择正确版本。

**macOS 提示找不到应用。** 打开访达的 **应用程序** 文件夹，确认应用名称确实是 `NexFilm Engine.app`。如果你把它放在了桌面、下载目录或其他位置，需要把终端命令里的路径改成实际路径；如果应用名称被系统自动改成了 `NexFilm Engine 2.app`，也要一起修改。

**执行命令后仍然无法打开。** 关闭应用窗口，再执行一次命令，然后重新打开。若系统仍然提示安全拦截，先确认应用确实位于应用程序目录，并查看 [故障排查与附录](/zh/docs/troubleshooting-and-appendix/) 中关于启动和文件路径的部分。不要通过关闭 SIP 来解决单个应用的启动问题。

**安装完成后不知道从哪里开始。** 下一步可以阅读 [认识 NexFilm：原理、安装与工作区全览](/zh/docs/overview-and-install/)，或者直接跟着 [第一次走通整卷流程](/zh/docs/first-roll-in-five-minutes/) 做一遍端到端练习。如果你还没有开始翻拍底片，建议先看 [相机翻拍与扫描仪前期采集规范](/zh/docs/scanning-best-practices/)。
