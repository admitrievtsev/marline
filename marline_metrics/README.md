# `marline_metrics`: внедрение и использование метрик

`marline_metrics` — общий crate для сбора latency-метрик по стадиям Marline. Он хранит агрегаты вместо отдельных измерений, поэтому стоимость памяти не растёт с количеством обработанных чанков. Измерения сначала накапливаются в локальном `MetricsBatch`, а затем объединяются с recorder при уничтожении batch.

Это руководство описывает API crate.

## 1. Подключить crate

Чтобы использовать recorder API отдельно, добавьте `marline_metrics` в `Cargo.toml`:

```toml
[dependencies]
marline_metrics = { path = "../marline_metrics" }
```

Чтобы вызывать `SBCScrubber::with_metrics`, включите feature `runtime-metrics` у `marline_scrub`: без неё этот метод и код instrumentation не компилируются. Если instrumentation должна быть включена без отдельного feature в вашем приложении, зависимость можно объявить так:

```toml
[dependencies]
marline_scrub = { path = "../marline_scrub", features = ["runtime-metrics"] }
marline_metrics = { path = "../marline_metrics" }
```

Если приложение должно уметь компилироваться как с instrumentation, так и без неё, сделайте обе зависимости optional и прокиньте feature явно. У `marline_scrub` `runtime-metrics` выключен по умолчанию:

```toml
[dependencies]
marline_scrub = { path = "../marline_scrub", default-features = false }
marline_metrics = { path = "../marline_metrics", optional = true }

[features]
default = []
runtime-metrics = ["dep:marline_metrics", "marline_scrub/runtime-metrics"]
```

Когда код использует API метрик, оградите его тем же feature приложения, чтобы он собирался при выключенном instrumentation:

```toml
[features]
runtime-metrics = ["dep:marline_metrics", "marline_scrub/runtime-metrics"]
```

Например, workspace-бинарник `marline` включает этот feature по умолчанию, но позволяет выключить его через `--no-default-features`. Не включайте default features зависимости безусловно, если нужно, чтобы отключение feature в приложении действительно исключало instrumentation.

## 2. Добавить стадию

Стадии объявляются в enum `Stage` в `marline_metrics/src/lib.rs`. Для новой стадии нужно:

1. Добавить вариант в `Stage`.
2. Добавить его в `Stage::ALL` и увеличить длину массива.
3. Добавить стабильное имя в `Stage::as_str()`.
4. Добавить тест на имя и, если нужно, на сбор измерений.

Например, новая стадия `MyFeatureLookup` должна получить имя вроде `"my_pipeline.feature_lookup"`.

**Добавляйте новые варианты в конец enum.** Discriminant используется для индексации массива агрегатов и битовой маски включённых стадий; перестановка существующих вариантов меняет их индексы. Сохраняйте `Stage::ALL` в соответствии с discriminant-ами и ограничивайте количество стадий 64, поскольку recorder использует `u64` как битовую маску.

`Stage::as_str()` — внешний идентификатор стадии в CSV/JSON и отчётах. После публикации его следует считать стабильным: не переименовывайте уже используемые имена без необходимости обновить downstream-инструменты и данные.

## 3. Инструментировать операцию

Для операции, которую удобно передать closure, используйте `MetricsBatch::measure`:

```rust
use marline_metrics::{MetricsRecorder, Stage};

fn example(recorder: &MetricsRecorder) {
    let mut batch = recorder.batch();

    let result = batch.measure(Stage::PalantirFingerprintLookup, || {
        lookup_fingerprint()
    });

    // batch объединит собранные измерения при выходе из scope.
    drop(batch);
    let _ = result;
}

fn lookup_fingerprint() -> Option<u64> {
    None
}
```

Если таймер должен охватывать несколько выражений или завершаться после `?`, проверяйте feature/runtime-настройку, снимайте время вручную и записывайте его в batch:

```rust
use std::time::Instant;

use marline_metrics::{MetricsRecorder, Stage};

fn example(recorder: &MetricsRecorder) -> std::io::Result<()> {
    let mut batch = recorder.batch();
    let enabled = recorder.is_enabled(Stage::PalantirCompression);
    let start = enabled.then(Instant::now);

    let result = compress_data();

    if let Some(start) = start {
        // Записываем, даже если настройку стадии изменили после старта таймера.
        batch.record_unconditionally(Stage::PalantirCompression, start.elapsed());
    }

    // Явный drop гарантирует merge до возможного snapshot ниже.
    drop(batch);
    result?;
    Ok(())
}

fn compress_data() -> std::io::Result<()> {
    Ok(())
}
```

Выбор API:

- `measure(stage, operation)` — измеряет closure, если стадия включена при входе; иначе только вызывает её.
- `record(stage, duration)` — записывает готовую длительность, только если стадия включена непосредственно при записи.
- `record_unconditionally(stage, duration)` — записывает без повторной проверки. Используйте, если уже приняли решение начать таймер по `is_enabled()`.

Для accounting-подобного отчёта заведите отдельную стадию остатка (unattributed), вычисленную как total минус неперекрывающиеся дочерние интервалы.

## 4. Создать recorder, включить стадии и прочитать результаты

Recorder можно клонировать и передавать в scrubber: клоны указывают на одно общее состояние. Пример ниже требует, чтобы зависимость `marline_scrub` была собрана с feature `runtime-metrics` (см. раздел «Подключить crate»).

```rust
use marline_metrics::{MetricsRecorder, Stage};

let recorder = MetricsRecorder::disabled();
recorder.set_enabled(Stage::SbcHashing, true);
recorder.set_enabled(Stage::SbcEncoding, true);

// Либо включить все известные стадии:
// let recorder = MetricsRecorder::enabled_all();

let scrubber = SBCScrubber::new(hasher, clusterer, encoder)
    .with_metrics(recorder.clone());

// Запустить pipeline с scrubber, затем получить агрегаты.
let snapshot = recorder.snapshot();
for metric in snapshot.stages {
    println!(
        "{}: count={}, total={:?}, average={:?}, min={:?}, max={:?}, p99={:?}",
        metric.stage.as_str(),
        metric.count,
        metric.total,
        metric.average,
        metric.min,
        metric.max,
        metric.p99(),
    );
}
```

В примере `SBCScrubber`, `hasher`, `clusterer` и `encoder` нужно заменить конкретными типами приложения; импортируйте сам `SBCScrubber` из `marline_scrub`.

Методы recorder:

| API | Назначение |
| --- | --- |
| `MetricsRecorder::disabled()` | Создать recorder с выключенными стадиями (также поведение `Default`). |
| `MetricsRecorder::enabled_all()` | Включить все стадии, известные этой версии crate. |
| `set_enabled(stage, bool)` | Включить/выключить одну стадию, в том числе во время работы. |
| `set_all_enabled(bool)` | Изменить состояние всех стадий. |
| `is_enabled(stage)` | Проверить, будут ли начинаться новые измерения. |
| `batch()` | Создать локальный batch измерений. |
| `snapshot()` | Получить согласованный снимок накопленных агрегатов. |
| `clear()` | Очистить агрегаты, не меняя включённые стадии. Batch, созданные до очистки, будут отброшены при последующем `Drop`. |

`StageMetrics` содержит `count`, `total`, `average`, `min`, `max` и приближённые `p50()`, `p90()`, `p99()`. Перцентили — верхние границы bucket-ов гистограммы с фиксированной памятью.

Batch сливает локальные агрегаты в recorder в `Drop`. Если snapshot нужен сразу после вызова, уничтожьте batch явно через `drop(batch)` до `snapshot()`. Snapshot содержит только стадии, для которых записано хотя бы одно измерение.

`clear()` инвалидирует уже созданные batch: их локальные samples не попадут обратно в очищенный recorder. Batch, созданный после очистки, агрегирует данные как обычно.

Для бинарника workspace можно отключить recorder вместе со всем instrumentation командой `cargo run -p marline --no-default-features`. По умолчанию пакет `marline` включает `runtime-metrics`, но сбор самих samples включается только переменной окружения `MARLINE_METRICS`.
