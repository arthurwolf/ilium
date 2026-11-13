## Form row leaders

Form rows link label to control with grey `…` leaders. `value_control::ValueControl::render` derives them from the ink rects of label text, buttons and value, so every shared stepper or choice row gets them without per-screen code. Text-built rows (`settings_ui`, `cost_settings_ui`, `remote_compaction_settings_ui`, `animation_settings_ui` value rows) call `value_control::leader_span` for the label-to-control gap. Both paths use one rule: a blank cell on each side and no dots in gaps shorter than five cells. The dots are presentation only and never part of a hit target.
