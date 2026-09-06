## 人狼型の秘匿役職盤面 (上級・任意)

```yaml
role_assignment:                    # 役職のランダム割り当て (エンジンが決定論 shuffle)
  key: 役職
  pool: { 人狼: 2, 占い師: 1, 村人: 3 }
  among: [player, mira, gen, sayo, tokio, yuren]   # 人数は pool 合計と一致必須
secret_attributes: [役職]            # 宛先別秘匿 (GM は全員分見えるが演じ分ける。本人は自分の分を見える)
hidden_attributes: [真の正体]         # 本人にも見えない属性 (自覚のない正体・呪い等)。UI は本人分ごと隠し、GM は「当人にも明かさない」規律で扱う。キーの宣言必須は secret と同じ
vote_rules:                         # 投票権の宣言 (書かなければ誰も投票できない)
  - { when: { kind: flag_is, key: 投票中, value: true } }
```

生存管理 (`生存` stat・`生存{役職}数`・`{役職}優位`) は role_assignment が自動生成し、開票 (`resolve_vote`) だけが更新します。フェーズ進行は repeatable トリガー + タイマー (record_turn / turns_since) で書きます。
