# 服务端判定面：能读到什么、怎么用

来源：Nyx4 的《服务端黑盒探测报告_判定机制》《查证接口全量审计报告》（2026-10），
以及本仓库 `run` 提交后回读的实测输出。本文只记**可复现的结论**，不含推测。

## 一、判定结果全部可读

`POST /api/v70320/runnings/get_one_record {rrid}` 返回 53 个字段，其中判定相关的：

| 字段 | 语义 | 正常值 |
|---|---|---|
| `isCheat` | 作弊标志 | 0 |
| `stepFreqStatus` | 步频判定 | 0 |
| `unCompleteReason` | 未完成原因码 | 0 |
| `status` | 记录状态分类（App 记录列表的标签） | 0 |
| `statusInfo` | 判定文案 | "太棒了，恭喜您完成了跑步…" |
| `validDis` / `validTime` | **服务端算的有效**距离/时间 | = totalDis / totalTime |
| `reasonList` | 逐条规则裁定 | 见下 |
| `appealStatus` / `canAppeal` | 申诉状态 | 1 / 1 |

`run` 现在提交完会立刻回读这几项并逐条打印（`[verify]` 前缀）。这是本地自检
替代不了的——自检再全也只是猜。

## 二、reasonList：逐条规则的裁定

```json
{"complete": true, "completeStatus": 1, "type": 1,
 "reason": "最少要跑1.50公里",
 "iconUrl": "https://h5.gxapp.iydsj.com/gxapp/icon/running/..."}
```

实测某学校（齐齐哈尔医学院）的六条：

```
✓ 经过必经点位
✓ 至少通过2个普通点位
✓ 最少要跑1.50公里
✓ 跑12分钟以上
✓ 步频控制在40步/分钟-300步/分钟之间
✓ 配速控制在2′00″-12′00″
```

注意：**「必经点位」和「普通点位」是分开判的**，普通点还要「至少通过 2 个」。

## 三、记录状态枚举

```
0 正常 · 1 作弊 · 2 未打卡（点位）· 3 未上传 · 4 配速过快 · 5 步频过慢
```

`status ≠ 0` 就是被归了类，成绩大概率不算。

## 四、违规与冻结

```
可疑 N 次 → 作弊 N 次 → 处罚（封禁 / 暂停 / 冻结）
                              ↓
                  policy.runRuleModel.freezeRunTime > 0 = 冻结中
                              ↓
                  到期自动解冻 → 申诉（v70106/runnings/appeal/submit）
```

- `freezeRunTime`（毫秒戳）> 当前时间 = 冻结期。**冻结期提交等于自己再记一次违规**，`run` 现在会直接拒绝。
- `POST /api/v78/cheat/cheatlist` 的 `self` 非 null = 已被标记。很多判定是事后跑批的，「提交成功」不代表干净，`run` 现在提交前会先查。

## 五、月度核验 = 支付宝芝麻信用

不是人脸。`GET /api/v70390/creditscore/popup?taskType=1` → `{needPopup}`。
命中要做真人核验，不做那条成绩可能被判不达标。自动化替不了，`run` 只提醒。

人脸是另一条线（3 次失败或超时 → 成绩不达标），接口 `v70100/run/facecheck/save`。

## 六、3 天补传窗口

App 文案：「成绩已暂时保存至本地，请于3天内上传成绩」。

提交失败**不等于白跑**。真正麻烦的是重复提交——先核对那条入账没有。

## 七、发送 vs 存储的归一化（解释「明明发对了」）

| 字段 | 发 | 存 | 说明 |
|---|---|---|---|
| `speed` | 7500 | 7.5 | ÷1000，单位是「分/公里」 |
| `selDistance` | 1200 | 1.0 | 服务端存**整数公里**；我们发米是对的 |
| `policy` / `status` | 1 | 0 | 服务端归一化覆盖 |
| `fivePointJson` | 5 个点 | `""` | 自由跑被剥离；计分跑另说 |
| `allLocJson` | 全轨迹 | null | 转进对象存储（`jsonFromObs=1`） |

## 八、考勤政策（getHistoryConfig）

```
mode=METER · maleDayUpper=5000 femaleDayUpper=5000（日上限）
maleMinSingleDis=1000 femaleMinSingleDis=1000（单次下限）
randomValidTimes=[{startTime:"06:30:00", endTime:"22:00:59"}]  ← 字段名就叫 random
```

时段字段名是 `randomValidTimes`，不是 `validTimes`。

## 九、给自检的启示

`reasonList` 里每一条 `complete=false` 都是**服务端说的**，比任何本地阈值都准。
遇到「服务端有意见」时先看这条清单，不要再去猜区间。
