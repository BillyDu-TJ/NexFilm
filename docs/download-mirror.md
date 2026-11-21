# 下载镜像

官网默认使用 `https://dl.nexfilm.tech` 作为 Release 安装包的镜像地址。GitHub Release 仍是权威源，镜像只负责把同一批资产复制到 Cloudflare R2，并在网站上保留 GitHub 直链作为兜底。

## Cloudflare 配置

1. 在 R2 创建一个 bucket，例如 `nexfilm-downloads`。
2. 给 bucket 绑定自定义域名 `dl.nexfilm.tech`，并保持公开读取。
3. 创建只授权该 bucket 的 R2 API Token，权限选择 Object Read & Write。
4. 在 GitHub 仓库 Actions secrets 中添加：
   - `R2_ACCOUNT_ID`
   - `R2_ACCESS_KEY_ID`
   - `R2_SECRET_ACCESS_KEY`
   - `R2_BUCKET`
5. 可选：在 Actions variables 中添加 `R2_PUBLIC_BASE_URL`，默认值为 `https://dl.nexfilm.tech`。

## 同步已经发布的版本

在 GitHub Actions 手动运行 `Mirror release assets`，标签填目标版本，例如 `v1.2.1`。也可以在本机配置同样的环境变量后执行：

```powershell
node scripts/sync-release-to-r2.mjs --tag v1.2.1
```

只检查下载、SHA256 和待上传路径而不写 R2：

```powershell
node scripts/sync-release-to-r2.mjs --tag v1.2.1 --dry-run
```

脚本只同步三个安装包，并同时生成：

- `latest.json`
- `<tag>/latest.json`

安装包使用长期不可变缓存，`latest.json` 使用 `no-cache`。每次同步都会校验 GitHub API 返回的 SHA256，发现不一致时停止。

## 上线顺序

官网默认使用镜像地址，所以要先准备 R2，再部署官网代码，避免短时间出现 404：

1. 创建 R2 bucket、绑定 `dl.nexfilm.tech`、配置 GitHub secrets。
2. 在 Cloudflare Pages 暂时把 `PUBLIC_DOWNLOAD_MIRROR_BASE` 设为空字符串，保持官网使用 GitHub 直链。
3. 推送代码，运行 `Mirror release assets` 同步 `v1.2.1`。
4. 用下面的验证命令确认安装包和 `latest.json` 可访问。
5. 把 Cloudflare Pages 的 `PUBLIC_DOWNLOAD_MIRROR_BASE` 改回 `https://dl.nexfilm.tech` 并重新部署官网。

## 官网配置

Cloudflare Pages 环境变量：

```text
PUBLIC_DOWNLOAD_MIRROR_BASE=https://dl.nexfilm.tech
```

将该变量设为空字符串会关闭镜像，官网回到 GitHub 直链。

## 验证

同步完成后检查一个安装包和一个清单：

```powershell
curl.exe -I https://dl.nexfilm.tech/v1.2.1/NexFilm.Engine_1.2.1_x64-setup.exe
curl.exe https://dl.nexfilm.tech/latest.json
```

官网构建后运行：

```powershell
node website/scripts/verify-download-mirror.mjs
```
