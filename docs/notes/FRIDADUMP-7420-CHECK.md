# 第三方 7.4.20 脱壳包的对照结果（Inverx / FridaDump）

一次**独立来源**的 7.4.20 脱壳包落到手上，与 `REVERSE-BASELINE.md`（2026-11-23，
自采 `libapp.so` + 原生 so + 壳 dex 三路）不是同一批数据。本文只记"它证实了什么、
和仓库现有结论冲突了什么、它没有覆盖什么"，不再复述已知协议。

包来源：`github.com/Inverx/com.zjwh.android_wh_physicalfitness` →
`原始脱壳资料/运动世界7.4.20-FridaDump.zip`（27.0 MB）。

## 一、包内构成

| 内容 | 说明 |
|---|---|
| `unpacked_dex/classes{,2..8}.dex` | 8 个，共 73 MB，**明文主体**；合计 69,935 类、412,137 个完好的 Dalvik 方法、3,989 个 `ACC_NATIVE` |
| `manifest.json` | 壳 dex 的 carve 记录：`payload_start_offset=0x3d930`、`nis_4096_keystream.bin` 的 sha256、逐 dex 的 `methods_native` / `zjwh_methods_native` |
| `unpacked_dex/zjwh_native_methods.txt` | `com/zjwh` 被抽成 native 的方法清单：**classes7 里 316 类 / 2,354 个方法**，`[0x112]` 标记 |
| `unpacked_so/libnesec-x86-unpacked.so` | 易盾 NetSecKit 的 JNI 后端，`Java_*` 导出 2,963 个 |
| `zjwh_native_methods.txt` + `libnesec_java_exports.txt` | 上面两份的索引 |

`manifest.json` 里每个 dex 的 `orig_sha1_matched` / `orig_adler_matched` 都是 true，
说明脱壳后的 dex 结构自洽（不是随手拼的字符串桶），可以直接当反编译输入用。

**版本核对**：8 个 dex 的字符串表里 `7.4.20` 唯一命中，与 `api/client.rs` 的
`appVersion` 默认值一致。

## 二、证实了的（逐项）

### 1. 签名算法：与 `crypto/sign.rs` 逐字节一致

链路在 Dalvik 层完整可读，不需要 so：

```
HttpUtil.signRunRecord(UploadFormatEntity, Z)
 └─ new UploadSignEntity(entity).OooO00o()        ← 反射拿 31 个字段
 └─ [flag] map.put("roomId", entity.getRoomId())
 └─ entity.setOriginalSign( o00O00O.OooO0Oo(map) )   ← 无盐
 └─ entity.setSignature(    o00O00O.OooO0oO(map) )   ← 加盐 MD5
 └─ return JsonUtils.OooO00o(entity)                 ← body JSON
```

`utils/o00O00O` 的四个方法语义（按字节码逐条读）：

| 方法 | 行为 |
|---|---|
| `OooOO0(Map)` | 过滤：**丢值为 null 的键**、丢 `equalsIgnoreCase("signature")` 的键；空串**保留** |
| `OooO00o(Map)` | `Collections.sort()` 自然序 → `k=v&k=v`，**不加盐** |
| `OooO0O0(Map, salt)` | 比较器 `o00O00O$OooO00o` → `compareToIgnoreCase` → 拼接 → **直接追盐**（无分隔符） |
| `OooO0OO(String)` | MD5 → 小写 hex |
| `OooO0Oo(Map)` | `OooOO0` + `OooO00o` → **originalSign**（无盐、区分大小写排序） |
| `OooO0o0(Map, salt)` | `OooOO0` + `OooO0O0` → 未哈希的 join+salt（`tokenSign()` 里叫 `preSign`） |
| `OooO0oo(Map, salt)` | `OooO0o0` 的结果再 MD5 → **signature** |
| `OooO0oO(Map)` / `OooO(Map)` | 取盐 `SportJniUtils.getSecretKey(5)` 后走上两行 |

对照 `crypto/sign.rs`：`sign_with_salt`（去 None、去 `signature`、小写序、追盐、MD5 小写）
与 `original_sign`（去 None、`cmp` 自然序、无盐）**语义一致**。唯一差异是排序比较器：
App 用 `compareToIgnoreCase` 单键，我们用「小写 + 原串」双键——只有存在仅大小写不同的
同名键时才会分叉（现有 31 个字段不可能出现），**不改**，但注释里别再写"双键排序是服务端
要求的"，它不是。

### 2. 签名域 = 31 个字段，`goalId` 发字符串 `"null"`

`entity/UploadSignEntity` 的 `private` 字段完整清单（反射 `getDeclaredFields()`，
`setAccessible(true)` 后逐个 `Field.get`）：

```
address avgPower avgStepFreq calorie complete errorCode faceCheck geeToken
getPrize goalId policy selDistance selRunTime selectedUnid speed sportType
startTime status stopTime themeId totalAscent totalDis totalSteps totalTime
uid unCompleteReason unauthorized useMobilityTools uuid validDis validTime
```

与 `api/submit.rs` 的 `sign_fields` **31/31 对齐**，一个不多一个不少（`roomId` 只在
`flag=true` 时补，见下）。注意 `totalDis` / `validDis` / `validTime` / `speed` 在实体里是
`long`，业务实体 `UploadFormatEntity.totalDis` 是 `int`——签名侧统一按 long 转字符串，
数值不变。

`goalId` 为 `Integer` 且为 null 时的处理值得单独钉一下，因为它是"直觉上会被过滤掉、
实际不会"的那种：

```
instance-of v6, v5, Ljava/lang/String;      # 不是 String
invoke-static v5, Ljava/lang/String;->valueOf(Ljava/lang/Object;)Ljava/lang/String;
invoke-interface v1, v4, v5, Map;->put(..)  # "goalId" -> "null"
```

`String.valueOf((Object)null)` 返回字符串 `"null"`，所以它**不是** null 值，
`OooOO0` 的过滤不丢它。签名域里必须出现 `goalId=null`。这一条与 `api/submit.rs`
现有写法（签名侧固定 `"null"`、body 侧 JSON null）完全一致，可以当作已证。

### 3. `tokenSign` 与 `preSign`

`hybrid/channel_runtime/net/NativeNetworkGatewaySign.tokenSign(uid, token, timeStamp)`：

- 组成 `{timeStamp, token, uid}` 三键 → `o00O00O.OooO0oO(map)` = **tokenSign**；
- 同一方法还把 `OooO0o0(map, salt)` 作为 **preSign** 一起返回（未哈希的 join+salt）。

native okhttp 路径只发了 `tokenSign`（`HttpUtil.getHeaderMap` 里只 put 了这一个）；
Flutter 桥路径（`NativeNetworkFlutterParamHeaders`）也只拼 `tokenSign` + `nonce`。
`preSign` 到底有没有作为 HTTP 头上行，取决于 `libapp.so` 的 Dart 层——**本包不含
libapp.so，未定**（见「四、未覆盖」）。

### 4. 请求头集合

`HttpUtil.getHeaderMap(Context)`（native okhttp 路径）实际拼的键：

```
Accept, Content-Type, appVersion,
osType(=0), DeviceId, osVersion, deviceName, IMEI, logicPixel, physicPixel,
androidId, blMac, wifiMac, cpuModel, isRoot, appUpdateTime, appInstallTime,
nonce, uid, token, timeStamp, tokenSign, CustomDeviceId
```

`CustomDeviceId` 空时用 `Utils.OoooOo0(context, String.valueOf(System.currentTimeMillis()))`
派生——与 `api/client.rs` 的 `hw_id` 派生/清空开关是同一层设计，值不同不影响。

`NativeNetworkFlutterParamHeaders.fullCommonParamHeaders()` 另有一套更短的集合
（`Accept`/`Content-Type`/`appVersion` + `getHeaderMap()` 的并集），说明 7.4.x 存在
**双请求栈**：Dalvik okhttp 老栈与 `hybrid/channel_runtime` 的 Dart 桥。pace-forge 走的是
老栈那套头，正确。

### 5. `isPass` 恒 true

`utils/sport/plugin/FivePointPlugin` 两处命中回调里：

```
const/4 v0, 1
invoke-virtual v2, v0, FivePoint;->setIsPass(Z)V
```

硬编码 `true`，不依赖任何测量值——"必到点在本地就被标成已通过"这条成立。

### 6. `save/record` 端点未变

字符串表里的 runnings 族共 19 条，其中 `api/v70260/runnings/save/record` 原文存在，
与 `api/submit.rs` 用的路径一致。

## 三、和仓库现有结论冲突 / 需要复核的

### `more: true`

`setMore` 在 8 个 dex 里只找到一个调用点：

```
mvi/run/mode/RunHistoryDetailPImpl$OooOOO0.call()Lokhttp3/RequestBody;
  ... RunHistoryDetailBean;->isMore()Z
  ... UploadFormatEntity;->setMore(Z)V
```

即**补传/详情重发**路径，取值来自服务端返回的 bean。正常运行路径（`CreditRunPIml` /
`BaseRunPImpl` 的 `getUploadJson`）里没有 `setMore`，`more` 保持 `boolean` 默认 `false`。

这和 `api/submit.rs` 里"真机是 true（脱壳确认，7.4.0/7.4.20 一致）"的注释冲突。
**不能就此下结论说应该改回 false**：classes7 里 `com/zjwh` 有 2,354 个方法被抽成
native（`libnesec` 的 VM 执行），写入点完全可能在抽取掉的那部分里，本包看不出来。
处理方式：保留 `true`，把注释里的依据改成"两份脱壳证据冲突，等真机报文"，别再写成
"脱壳确认"。

### `isFixed` 的写入点

`FivePoint.isFixed` 是 `int`（`RunPoint.isFixed` 是 `boolean`，两个类不是一回事），
本包内没有找到任何 `setIsFixed` 调用点，取值来自服务端下发的点位对象。这与
`isFixed` 只影响 App 端画点颜色、服务端自己判经过的结论相容，但同样受 native 抽取盲区
影响，不作为"必发 0"的证明。

## 四、这个包没有覆盖的

1. **盐的真实来源**。`NativeNetworkGatewaySign.salt()` = `SportJniUtils.getSecretKey(5)`，
   `SportJniUtils.<clinit>` 里 `System.loadLibrary("swsport")` → **`libswsport.so`**。
   本包只解了 `libnesec-x86-unpacked.so`，`libswsport` 不在内。
   `SIGN_SALT` / `KEYDATA_TAIL` / `ABCDEFGHIJKLMN12` / 两把 RSA 公钥 / `keyData*` / `"101"`
   在 8 个 dex 的字符串表里 **0 命中**——继续按 `crypto/consts.rs` 顶部写的
   【实机联调】层级引用，不要说"从 so 里读出来的"。
2. **请求加解密实现**。`net/ApiCrypto` 的 4 个方法（`decryptResponse` / `encryptBody` /
   `encryptHeaderSign` / `liteHeaders`）全是 `ACC_NATIVE`，`hybrid/.../NativeNetworkSecurityContract`
   同样三个方法 native；`NativeNetworkGatewayCipher` 只剩 Gson 组 JSON + 调
   `ISecretManager.encryptStringToServer`。信封逻辑整体在 native（`libnesec` 的 VM 里），
   `libnesec_java_exports.txt` 只有 trampoline。**信封参数继续按经验值。**
3. **Dart 层**（`libapp.so`）：端点前缀（`v70240` / `v70260` 混用）、`preSign` 是否上行、
   采样桶的实际拼接，这包都看不到。
4. 采样点距、OBS 对象的 10 个键、`geetoken` 的 `GT_KEY`：本包无增量，`REVERSE-BASELINE.md`
   的结论不变。

## 五、顺手捞到的、可能有用但还没接进来的东西

| 事实 | 出处 | 现状 |
|---|---|---|
| 第 5 个域名 `activity = http://api.physictool.iydsj.com`（**明文 http**） | `NativeNetworkBootstrap.buildConfig()` 的 `domains` 表 | `crypto/consts.rs::hosts` 只有 run/discovery/datapoint/partner |
| `environment` 取值：host 含 `pre.run.` → `187`，含 `test.gxapp.` → `185`，否则 `online` | 同上 | 未用；跑 `run.gxapp` 就是 `online`，不影响现有链路 |
| `api/runnings/upload/originalSign` | dex 字符串 | 未实现。看名字是"上传 originalSign"，可能用于补校验，值得单独探一次 |
| `api/v670/runnings/hardware/antCheating/details` + `entity/AntCheatingDetailsEntity{config, list}` | dex 字符串 / 实体 | 未实现。硬件反作弊详情，读回来能知道服务端记了什么 |
| `api/v68/runnings/refreshRecord`、`api/v70106/runnings/appeal/submit`、`api/v70101/runnings/report/share` | dex 字符串 | 未实现 |

## 六、结论

- 签名、签名域、`goalId="null"`、`isPass=true`、`save/record=v70260` 这五条**被独立
  来源复现**，可以按"已证"写。
- 盐 / 信封 / KDF 仍然只有经验值，这次没有改变证据等级。
- 唯二要改的是**注释口径**（`more`、`isFixed` 的依据从"脱壳确认"降级为"证据冲突"），
  不是代码。
- 域名表可以补一条 `activity`，但没有任何现有链路用到，先记在这。
