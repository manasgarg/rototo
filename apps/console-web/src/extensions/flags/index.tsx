// The flags experience (design/console-surfaces.md): bound bool variables
// and layers rendered as a flag list with derived status. Everything here
// goes through the extension contract — read, propose, and the ui kit — and
// anything the recognizers do not understand degrades per item to the
// workbench, never to a guess. This extension holds zero architectural
// privilege; if the contract cannot support it, the contract is wrong.

import { useState } from "react";

import type {
    EditOperation,
    ExperienceModule,
    ExperienceProps,
    SurfaceItem,
} from "../../extension-api.ts";
import {
    advanceRingOperation,
    deriveFlagStatus,
    dialOperations,
    nextRing,
    scheduleFlipOperation,
    type FlagStatus,
    type RolloutView,
} from "./logic.ts";

function Render(props: ExperienceProps) {
    const { surface, items, editable, now, propose, ui, openWorkbench } = props;
    const killSwitch = surface.approval === "none";
    const rings = ringsOf(surface.config);

    const flags = items.filter(
        (item): item is Extract<SurfaceItem, { kind: "variable" }> =>
            item.kind === "variable",
    );
    const layers = items.filter(
        (item): item is Extract<SurfaceItem, { kind: "layer" }> =>
            item.kind === "layer",
    );
    const extras = items.filter(
        (item) => item.kind !== "variable" && item.kind !== "layer",
    );

    return (
        <div className="section">
            {killSwitch ? (
                <ui.Banner tone="warn">
                    Kill-switch surface: changes here merge without approval.
                    Speed is the point; so is care.
                </ui.Banner>
            ) : null}
            {flags.map((flag) => (
                <Flag
                    key={flag.id}
                    flag={flag}
                    status={deriveFlagStatus(flag, now)}
                    rings={rings}
                    editable={editable}
                    propose={propose}
                    ui={ui}
                    openWorkbench={openWorkbench}
                />
            ))}
            {layers.map((layer) => (
                <div className="card" key={layer.id}>
                    <h3 className="mono">layer {layer.id}</h3>
                    {layer.description !== null ? (
                        <p className="hint">{layer.description}</p>
                    ) : null}
                    {layer.allocations.map((allocation, index) => (
                        <div className="field-row surface-item" key={index}>
                            <span className="label mono">
                                {allocation.id ?? `#${index}`}
                            </span>
                            <ui.Pill
                                tone={
                                    allocation.status === "running"
                                        ? "ok"
                                        : "neutral"
                                }
                            >
                                {allocation.status ?? "draft"}
                            </ui.Pill>
                            <span className="hint mono">
                                {allocation.arms
                                    .map(
                                        (arm) =>
                                            `${arm.name ?? "?"} ${arm.buckets ?? ""}`,
                                    )
                                    .join(" · ")}
                            </span>
                            {allocation.variables.length > 0 ? (
                                <span className="hint">
                                    drives {allocation.variables.join(", ")}
                                </span>
                            ) : null}
                        </div>
                    ))}
                </div>
            ))}
            {extras.map((item, index) => (
                <ui.AdvancedShape
                    key={index}
                    label={labelOf(item)}
                    detail="The flags experience renders flags and layers; this binding is something else."
                    onOpen={openWorkbench}
                />
            ))}
        </div>
    );
}

function Flag({
    flag,
    status,
    rings,
    editable,
    propose,
    ui,
    openWorkbench,
}: {
    flag: Extract<SurfaceItem, { kind: "variable" }>;
    status: FlagStatus;
    rings: string[];
    editable: boolean;
    propose: (operations: EditOperation[], summary: string) => void;
    ui: ExperienceProps["ui"];
    openWorkbench: () => void;
}) {
    if (status.state === "advanced") {
        return (
            <ui.AdvancedShape
                label={flag.id}
                detail={`Advanced shape (${status.reason}); the workbench shows it fully.`}
                onOpen={openWorkbench}
            />
        );
    }
    const tone =
        status.state === "on"
            ? "ok"
            : status.state === "partial"
              ? "info"
              : "neutral";
    const advance = nextRing(rings, status.rules);
    return (
        <div className="card">
            <div className="card-head">
                <div className="card-head-text">
                    <h3 className="mono">{flag.id}</h3>
                    <p className="hint">
                        {flag.description ?? ""}{" "}
                        <ui.Pill tone={tone}>{status.summary}</ui.Pill>
                    </p>
                </div>
                <ui.Toggle
                    on={flag.default === true}
                    disabled={!editable}
                    onChange={(next) =>
                        propose(
                            [
                                {
                                    op: "set_default",
                                    variable: flag.id,
                                    value: next,
                                },
                            ],
                            `${next ? "Launch" : "Turn off"} ${flag.id}`,
                        )
                    }
                />
            </div>
            {advance !== null && editable ? (
                <div className="action-row">
                    <ui.Button
                        tone="secondary"
                        title={`Add a rule turning ${flag.id} on for ${advance.ring}`}
                        onClick={() =>
                            propose(
                                [
                                    advanceRingOperation(
                                        flag.id,
                                        advance.ring,
                                        advance.position,
                                    ),
                                ],
                                `Advance ${flag.id} to ${advance.ring}`,
                            )
                        }
                    >
                        Advance to {advance.ring.replaceAll("_", " ")}
                    </ui.Button>
                </div>
            ) : null}
            {status.rollout !== null ? (
                <RolloutDial
                    flag={flag.id}
                    rollout={status.rollout}
                    editable={editable}
                    propose={propose}
                    ui={ui}
                />
            ) : null}
            <ScheduleFlip
                flag={flag.id}
                on={flag.default !== true}
                editable={editable}
                propose={propose}
                ui={ui}
            />
        </div>
    );
}

// The rollout dial: one slider over the treatment arm's share of the
// layer's buckets. Moving it emits set_arm_buckets (and keeps the control
// arm covering the rest); starting and concluding the rollout are the
// allocation's status.
function RolloutDial({
    flag,
    rollout,
    editable,
    propose,
    ui,
}: {
    flag: string;
    rollout: RolloutView;
    editable: boolean;
    propose: (operations: EditOperation[], summary: string) => void;
    ui: ExperienceProps["ui"];
}) {
    const [percent, setPercent] = useState(rollout.percent);
    const commit = () => {
        if (percent === rollout.percent) {
            return;
        }
        const buckets = Math.round((percent / 100) * rollout.totalBuckets);
        propose(
            dialOperations(rollout, buckets),
            `Roll ${flag} out to ${percent}%`,
        );
    };
    return (
        <div className="field-row surface-item">
            <span className="label">
                rollout{" "}
                <span className="mono">
                    {rollout.layer}/{rollout.allocation}
                </span>
            </span>
            <input
                type="range"
                min={0}
                max={100}
                value={percent}
                disabled={!editable || rollout.status === "concluded"}
                onChange={(event) => setPercent(Number(event.target.value))}
                onMouseUp={commit}
                onTouchEnd={commit}
                onKeyUp={(event) => {
                    if (event.key === "Enter") {
                        commit();
                    }
                }}
            />
            <span className="mono">{percent}%</span>
            <ui.Pill
                tone={
                    rollout.status === "running"
                        ? "ok"
                        : rollout.status === "concluded"
                          ? "neutral"
                          : "info"
                }
            >
                {rollout.status ?? "draft"}
            </ui.Pill>
            {rollout.status === "draft" ? (
                <ui.Button
                    tone="secondary"
                    disabled={!editable}
                    onClick={() =>
                        propose(
                            [
                                {
                                    op: "set_allocation_status",
                                    layer: rollout.layer,
                                    id: rollout.allocation,
                                    status: "running",
                                },
                            ],
                            `Start the ${flag} rollout`,
                        )
                    }
                >
                    Start
                </ui.Button>
            ) : null}
            {rollout.status === "running" ? (
                <ui.Button
                    tone="ghost"
                    disabled={!editable}
                    title="Everyone gets the default again"
                    onClick={() =>
                        propose(
                            [
                                {
                                    op: "set_allocation_status",
                                    layer: rollout.layer,
                                    id: rollout.allocation,
                                    status: "concluded",
                                },
                            ],
                            `Conclude the ${flag} rollout`,
                        )
                    }
                >
                    Conclude
                </ui.Button>
            ) : null}
        </div>
    );
}

function ScheduleFlip({
    flag,
    on,
    editable,
    propose,
    ui,
}: {
    flag: string;
    on: boolean;
    editable: boolean;
    propose: (operations: EditOperation[], summary: string) => void;
    ui: ExperienceProps["ui"];
}) {
    const [open, setOpen] = useState(false);
    const [instant, setInstant] = useState("");
    if (!open) {
        return (
            <div className="action-row">
                <ui.Button
                    tone="ghost"
                    disabled={!editable}
                    onClick={() => setOpen(true)}
                >
                    Schedule a flip
                </ui.Button>
            </div>
        );
    }
    return (
        <div className="inline-form">
            <input
                className="input"
                type="datetime-local"
                value={instant}
                onChange={(event) => setInstant(event.target.value)}
            />
            <ui.Button
                tone="primary"
                disabled={instant === ""}
                onClick={() => {
                    setOpen(false);
                    propose(
                        [scheduleFlipOperation(flag, `${instant}:00Z`, on)],
                        `Schedule ${flag} ${on ? "on" : "off"} at ${instant}`,
                    );
                }}
            >
                Schedule {on ? "on" : "off"}
            </ui.Button>
            <ui.Button tone="ghost" onClick={() => setOpen(false)}>
                Cancel
            </ui.Button>
        </div>
    );
}

function ringsOf(config: Record<string, unknown> | null): string[] {
    const rings = config?.rings;
    return Array.isArray(rings)
        ? rings.filter((ring): ring is string => typeof ring === "string")
        : [];
}

function labelOf(item: SurfaceItem): string {
    switch (item.kind) {
        case "variable":
        case "catalog":
        case "layer":
            return item.id;
        case "entry":
            return `${item.catalog}/${item.key}`;
        case "missing":
            return item.target;
    }
}

const flags: ExperienceModule = { kind: "flags", Render };
export default flags;
