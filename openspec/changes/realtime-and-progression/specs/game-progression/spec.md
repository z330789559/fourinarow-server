## ADDED Requirements

### Requirement: Load mode and level config at startup

系统 MUST 在启动时加载模式配置（`MinigameModeConfigData.json`）与关卡配置（`MinigameLevelConfigData.json`）到内存只读结构，并按 `mode` 建立索引、按关卡 `id` 在 mode 内升序排序、统计每个模式的关卡总数。配置缺失或解析失败时系统 MUST fail-fast（启动失败）。本期系统 MUST NOT 加载场景配置（`MinigameSceneConfigData.json`）。

#### Scenario: Configs loaded into memory on startup

- **GIVEN** 模式与关卡配置文件存在且格式合法
- **WHEN** 服务启动
- **THEN** 系统 MUST 将模式与关卡配置加载到内存
- **AND** 系统 MUST 可按 `mode` 查询其关卡列表（按 `id` 升序）与关卡总数

#### Scenario: Invalid config fails startup

- **GIVEN** 模式或关卡配置缺失或无法解析
- **WHEN** 服务启动
- **THEN** 系统 MUST 启动失败并报告配置错误

### Requirement: Ensure per-mode progress rows exist

系统 MUST 确保玩家在每个已知模式下都拥有一条进度记录，初始 `level = 0`（表示尚未完成任何关卡），其余进度字段预留。进度记录 SHOULD 在注册成功后由业务层经 `PlayerRepository` 初始化；由于建号与初始化非同一事务，若初始化缺失（失败或模式集合后续扩展），系统 MUST 在玩家开始游戏或完成关卡入口处对缺失模式懒补齐。进度写入 MUST 经玩家域（`PlayerRepository`）保持一致性。

#### Scenario: New player gets a progress row per mode

- **GIVEN** 配置中已知模式集合为 M
- **WHEN** 新玩家完成注册
- **THEN** 系统 SHOULD 为 M 中每个模式创建该玩家的进度记录
- **AND** 每条记录的初始 `level` MUST 为 0

#### Scenario: Lazily backfill missing progress rows

- **GIVEN** 某玩家缺少一个或多个已知模式的进度记录（注册初始化失败或模式集合扩展）
- **WHEN** 该玩家请求开始游戏或上报完成关卡
- **THEN** 系统 MUST 为其缺失模式补齐 `level = 0` 的进度记录后再继续处理

### Requirement: Start-game returns mode and level selection

系统 MUST 提供“开始游戏”协议（WS，JSON 子协议），返回：该玩家**可选模式**、**不可选模式**、**当前可选关卡**、以及**每个模式的关卡总数**。模式可选性 MUST 依据玩家在征途模式（mode 1）的 `level` 是否达到该模式的 `unlockByJourneyLevel`。关卡身份 MUST 以 `(mode, id)` 标识，玩家“当前可选关卡” MUST 为该模式 `level + 1` 对应的关卡 `id`。

#### Scenario: Return selectable and locked modes

- **GIVEN** 玩家征途模式 `level` 为 L
- **WHEN** 客户端发送开始游戏请求
- **THEN** 对每个模式，若 `L >= unlockByJourneyLevel` 系统 MUST 归入可选模式，否则归入不可选模式
- **AND** 响应 MUST 包含每个模式的关卡总数
- **AND** 响应 MUST 包含当前可选关卡（该模式 `level + 1` 对应的关卡 `id`）

### Requirement: Level card pagination

系统 MUST 支持按关卡 `id` 在 mode 内游标翻页获取下一批关卡卡片。关卡身份与排序 MUST 以 `(mode, id)` 为准；`levelMin` / `levelMax` / `chapter` 等仅作为卡片展示属性，MUST NOT 用作分页游标（经核对配置：同一 mode 内 `levelMin` 仅在 1–10 循环、非唯一，`id` 才是 mode 内连续唯一序号）。

#### Scenario: Fetch next page of level cards

- **GIVEN** 玩家在某模式下查看关卡卡片
- **WHEN** 客户端按 `{ mode, after_id, page_size }` 请求下一批
- **THEN** 系统 MUST 返回该 mode 下 `id > after_id` 的下一批关卡卡片（按 `id` 升序）

### Requirement: Complete-level via WS with server-side validation

系统 MUST 提供“完成关卡”协议（WS 上行，JSON 子协议）。服务端 MUST 依据玩家当前进度与关卡配置校验上报的合法性：模式 MUST 已解锁、上报的关卡 `(mode, level_id)` MUST 属于该模式且为玩家当前可完成的关卡（`level_id == 当前 level + 1`）、星数 MUST 在合法范围（1–3）。校验通过后系统 MUST 经 `PlayerRepository` 在同步事务内推进该模式 `level` 并发放奖励；校验失败 MUST 拒绝且不改变进度或发奖。完成关卡 MUST 产生一条 `complete_level` 行动事件（事务提交后异步记录，允许弱一致）。完成关卡操作 MUST 幂等。

#### Scenario: Valid completion advances progress

- **GIVEN** 玩家所在模式已解锁，且上报的 `level_id == 当前 level + 1`、星数在合法范围
- **WHEN** 客户端通过 WS 上报完成关卡
- **THEN** 系统 MUST 校验通过并将该模式的 `level` 推进为 `level_id`
- **AND** 系统 MUST 经 `PlayerRepository` 发放该关卡奖励
- **AND** 系统 MUST 产生一条 `complete_level` 行动事件

#### Scenario: Reject illegal completion

- **GIVEN** 上报的模式未解锁，或关卡不属于该模式，或 `level_id != 当前 level + 1`，或星数超出合法范围（非 1–3）
- **WHEN** 客户端上报完成关卡
- **THEN** 系统 MUST 拒绝该上报
- **AND** 系统 MUST NOT 改变玩家进度或发放奖励

#### Scenario: Completion is idempotent

- **GIVEN** 玩家已成功完成并推进某关卡
- **WHEN** 客户端重复上报同一关卡完成
- **THEN** 系统 MUST NOT 重复推进进度或重复发奖
