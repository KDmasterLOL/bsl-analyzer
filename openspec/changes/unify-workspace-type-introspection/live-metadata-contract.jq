def typed_items:
  [
    ((.["СтандартныеРеквизиты"] // [])
      + (.["Реквизиты"] // [])
      + (.["Измерения"] // [])
      + (.["Ресурсы"] // []))[]
    | select(has("type"))
  ];

typed_items as $items
| ($items | length) > 0
  and all($items[];
    (.typeVariants | type) == "array"
    and (.typeVariants | length) > 0
    and all(.typeVariants[]; has("technicalName") and has("presentation")))
