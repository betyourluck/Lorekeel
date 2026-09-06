#### トリガーから判定を起こす

**トリガーの effects には `attempt_challenge` / `attempt_contest` を書けます。** 筋書きの側から
ダイスを振らせる経路で、「罠が発動して回避判定」「時間経過で襲撃判定」のように、GM の提案を
待たずに判定を起こせます。

```yaml
triggers:
  - id: 落とし穴
    when: { kind: all, of: [{ kind: location_is, at: 回廊 }, { kind: flag_is, key: 床板を踏んだ, value: true }] }
    narration: 足元の床板が、鈍い音を立てて沈んだ。
    effects:
      - { op: attempt_challenge, entity: player, challenge: 回避 }
```

判定の中身 (ダイス・成功度・帰結スロット・結末文・効果音) は通常どおり全部効きます。

- ⚠ **`requires` は評価されません。** トリガーの効果は作者が書いたものとして信頼され、検証を
  通らないためです。GM が同じ判定を選ぼうとすれば `requires` で止まりますが、トリガーからは
  通ります。前提で縛りたいなら、トリガーの `when` の側に条件を書いてください。
- ⚠ **id を書き間違えると何も起きません。** トリガーは発火して narration だけ出て、判定が
  起きません。`attempt_contest` の場合はもっと厄介で、存在しない対決が「進行中」のまま居座り、
  以後の対決が全部拒否されます。**ロード時に警告が出ます**ので、開幕の ⚠ を必ず読んでください。
