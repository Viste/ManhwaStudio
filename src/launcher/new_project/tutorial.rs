/*
File: src/launcher/new_project/tutorial.rs

Purpose:
Branching tutorial for the "New project" window (`TutorialId::NewProject`). It
demonstrates the engine's branching (an intro that forks) and gating (steps that
trigger a real pipeline op and wait for it to finish).

The window's pipeline triggers are private `&mut self` methods, so the step
script cannot hold a reference to the window. Instead the tutorial context
`NpTutorialCtx` is a per-frame COMMAND SINK + STATE SNAPSHOT: `on_enter` pushes
`NpTutorialCommand`s (drained and executed by the window after `sync`), and gates
read the snapshot booleans. Keys here MUST match the `mark` calls in `window.rs`.

Two branches:
- Visual: download a test chapter, stitch+cut it, run waifu2x — each step waits
  for its op to finish before advancing.
- Explain: no processing; switch to the full panel and describe each section.
*/

use crate::tutorial::TutorialStep;

/// Per-frame context for the new-project tutorial: a snapshot the gates read plus
/// a command queue `on_enter` writes. Owned (no borrows of the window), so it can
/// be `C` in `TutorialController<NpTutorialCtx>`.
pub struct NpTutorialCtx {
    /// A pipeline op is running (`active_progress.is_some()`); gates wait on this.
    pub busy: bool,
    /// The ribbon has pages (a download/import produced something to process).
    pub ribbon_has_pages: bool,
    /// The waifu2x runtime is available (skip triggering it if not).
    pub waifu_available: bool,
    /// Actions requested this frame, executed by the window after `sync`.
    pub commands: Vec<NpTutorialCommand>,
}

/// An action the tutorial asks the window to perform. The window matches these on
/// `&mut self` after `sync` returns (so the tutorial never borrows the window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NpTutorialCommand {
    /// Show the step-based simple panel (where the test-chapter button lives).
    SwitchToSimple,
    /// Show the full panel (all sections visible at once for highlighting).
    SwitchToFull,
    /// Download the built-in test chapter.
    StartTestDownload,
    /// Stitch the ribbon and auto-cut it into pages.
    StartStitchAutoCut,
    /// Run the pages through waifu2x.
    StartWaifu2x,
}

// Target keys — must match `window.rs` `mark(...)` sites.
pub const TARGET_MODE_TABS: &str = "np_mode_tabs";
pub const TARGET_TEST_DOWNLOAD: &str = "np_test_download";
pub const TARGET_IMPORT: &str = "np_import";
pub const TARGET_QUICK: &str = "np_quick";
pub const TARGET_STITCH: &str = "np_stitch";
pub const TARGET_WAIFU: &str = "np_waifu";

/// Build the branching new-project tutorial.
#[must_use]
pub fn steps() -> Vec<TutorialStep<NpTutorialCtx>> {
    vec![
        // ---- Intro: fork on how to present the window ----
        TutorialStep::message(
            "Обучение — окно новой главы",
            "Показать наглядно на тестовой главе (скачаю, сошью, нарежу и обработаю \
             реальную главу) — или просто рассказать про кнопки без обработки?",
        )
        .id("np_intro")
        .choice("Показать наглядно", "np_vis_download")
        .choice("Просто рассказать", "np_exp_simple"),
        // ================= VISUAL BRANCH =================
        TutorialStep::new(
            [TARGET_TEST_DOWNLOAD],
            "Скачивание тестовой главы",
            "Нажимаю «Скачать тестовую главу» — качаю пример с comic.naver.com. \
             Это может занять время, дождёмся загрузки.",
        )
        .id("np_vis_download")
        .on_enter(|c: &mut NpTutorialCtx| {
            c.commands.push(NpTutorialCommand::SwitchToSimple);
            c.commands.push(NpTutorialCommand::StartTestDownload);
        })
        .await_gate(|g| !g.ctx.busy),
        TutorialStep::new(
            [TARGET_STITCH],
            "Склейка и нарезка",
            "Глава скачана. Теперь склеиваю вебтун-ленту и автоматически нарезаю \
             её на страницы.",
        )
        .id("np_vis_stitch")
        .on_enter(|c: &mut NpTutorialCtx| {
            c.commands.push(NpTutorialCommand::SwitchToFull);
            if c.ribbon_has_pages {
                c.commands.push(NpTutorialCommand::StartStitchAutoCut);
            }
        })
        .await_gate(|g| !g.ctx.busy),
        TutorialStep::new(
            [TARGET_WAIFU],
            "Обработка через waifu2x",
            "Прогоняю страницы через waifu2x — шумоподавление и апскейл. Так же \
             доступен Reline рядом в этой секции.",
        )
        .id("np_vis_waifu")
        .on_enter(|c: &mut NpTutorialCtx| {
            if c.waifu_available && c.ribbon_has_pages {
                c.commands.push(NpTutorialCommand::StartWaifu2x);
            }
        })
        .await_gate(|g| !g.ctx.busy),
        TutorialStep::message(
            "Готово!",
            "Тестовая глава скачана, сшита, нарезана и обработана. Отсюда её можно \
             сохранить как проект или экспортировать. Не обязательно сохранять — \
             окно можно использовать просто для выкачки и обработки.",
        )
        .id("np_vis_done")
        .finish(),
        // ================= EXPLAIN BRANCH =================
        TutorialStep::new(
            [TARGET_MODE_TABS],
            "Простой режим",
            "В простом режиме всё идёт по шагам — импорт, склейка, обработка, \
             сохранение. Обычно этого достаточно. Давайте посмотрим полную панель \
             со всеми инструментами сразу.",
        )
        .id("np_exp_simple"),
        TutorialStep::new(
            [TARGET_IMPORT],
            "Импорт",
            "Открыть папку или файл, вставить из буфера, режим захвата экрана — \
             сюда попадают исходники главы.",
        )
        .id("np_exp_import")
        .on_enter(|c: &mut NpTutorialCtx| c.commands.push(NpTutorialCommand::SwitchToFull)),
        TutorialStep::new(
            [TARGET_QUICK],
            "Выкачиватели",
            "Быстрый выкачиватель качает главу по ссылке с поддерживаемых сайтов. \
             Ниже — продвинутый: выкачка через подконтрольный браузер (Selenium \
             или CloakBrowser) для сложных случаев.",
        )
        .id("np_exp_quick"),
        TutorialStep::new(
            [TARGET_STITCH],
            "Сшивание и нарезка",
            "Склейка вебтун-ленты в одно полотно и нарезка обратно на страницы — \
             автоматически или с ручной расстановкой линий реза.",
        )
        .id("np_exp_stitch"),
        TutorialStep::new(
            [TARGET_WAIFU],
            "Обработка изображений",
            "waifu2x и Reline — шумоподавление и апскейл страниц перед переводом.",
        )
        .id("np_exp_process"),
        TutorialStep::message(
            "Это всё",
            "Полная панель даёт доступ ко всем инструментам сразу. Обработанную \
             главу можно сохранить как проект или использовать окно просто как \
             комбайн для выкачки и предобработки.",
        )
        .id("np_exp_done")
        .finish(),
    ]
}
