### Gate (条件) の語彙

<!-- vocab:gate -->

`not` は**どの条件でも否定できます**。「〜を持っていない」「〜がまだ立っていない」「その場所に
いない」を、フラグでの二重管理なしに直接書けます。

```yaml
# お守りを持っていなければ襲われる
when:
  kind: all
  of:
    - { kind: location_is, at: 森 }
    - { kind: not, of: { kind: has_item, item: お守り } }
```

`flag_is` の `value: false` や `stat_at_most` でも部分的に否定は書けますが、あれは個別の条件に
生えた反対語であって、`has_item` や `has_skill` には対応物がありません。`not` はその一般形です。
