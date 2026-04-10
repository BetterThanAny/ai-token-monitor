import { useMemo } from "react";
import type { CSSProperties } from "react";
import type { GridCell, GridRow } from "../hooks/useLeaderboardGrid";
import { useI18n } from "../i18n/I18nContext";
import { useMiniProfile } from "../contexts/MiniProfileContext";
import { toLocalDateStr } from "../lib/format";

interface Props {
  gridData: GridRow[];
  loading: boolean;
  userId: string;
}

type DeltaKind = "up" | "down" | "same" | "new";

interface AnnotatedCell extends GridCell {
  deltaKind: DeltaKind;
  deltaValue: number; // absolute value; 0 for same/new
}

const TOP_N = 10;

const CELL_SIZE = 54;
const AVATAR_SIZE = 33;
const DATE_COL_WIDTH = 56;

export function LeaderboardGrid({ gridData, loading, userId }: Props) {
  const t = useI18n();
  const { open: openMiniProfile } = useMiniProfile();

  // Compute rank delta for each cell by comparing with the next row (previous day).
  // gridData is already sorted by date desc.
  const annotated = useMemo(() => {
    return gridData.map((row, rowIdx) => {
      const prevRow = gridData[rowIdx + 1]; // previous day
      const prevRankById = new Map<string, number>();
      if (prevRow) {
        for (const e of prevRow.entries) prevRankById.set(e.user_id, e.rank);
      }

      const cells: AnnotatedCell[] = row.entries.map((cell) => {
        if (!prevRow) {
          // Oldest day in the window — no prior context, treat as "same"
          return { ...cell, deltaKind: "same", deltaValue: 0 };
        }
        const prevRank = prevRankById.get(cell.user_id);
        if (prevRank === undefined) {
          return { ...cell, deltaKind: "new", deltaValue: 0 };
        }
        const diff = prevRank - cell.rank; // positive = moved up
        if (diff > 0) return { ...cell, deltaKind: "up", deltaValue: diff };
        if (diff < 0) return { ...cell, deltaKind: "down", deltaValue: -diff };
        return { ...cell, deltaKind: "same", deltaValue: 0 };
      });

      return { date: row.date, entries: cells };
    });
  }, [gridData]);

  const hasAnyData = annotated.some((r) => r.entries.length > 0);

  return (
    <div style={{
      background: "var(--bg-card)",
      borderRadius: "var(--radius-lg)",
      padding: 10,
      boxShadow: "var(--shadow-card)",
      overflowX: "auto",
    }}>
      {loading && !hasAnyData ? (
        <div style={{
          padding: 20,
          textAlign: "center",
          color: "var(--text-secondary)",
          fontSize: 12,
          fontWeight: 600,
        }}>
          {t("leaderboard.loading")}
        </div>
      ) : !hasAnyData ? (
        <div style={{
          padding: 20,
          textAlign: "center",
          color: "var(--text-secondary)",
          fontSize: 12,
          fontWeight: 600,
        }}>
          {t("leaderboard.gridEmpty")}
        </div>
      ) : (
        <div style={{
          display: "inline-block",
          minWidth: "100%",
        }}>
          {/* Header row: rank labels */}
          <div style={{ display: "flex", alignItems: "center", marginBottom: 6 }}>
            <div style={{ width: DATE_COL_WIDTH, flexShrink: 0 }} />
            {Array.from({ length: TOP_N }, (_, i) => (
              <div
                key={i}
                style={{
                  width: CELL_SIZE,
                  textAlign: "center",
                  fontSize: 11,
                  fontWeight: 700,
                  color: "var(--text-secondary)",
                  flexShrink: 0,
                }}
              >
                {i + 1}
              </div>
            ))}
          </div>

          {/* Data rows */}
          {annotated.map((row) => (
            <div
              key={row.date}
              style={{
                display: "flex",
                alignItems: "center",
                marginBottom: 4,
              }}
            >
              <div style={{
                width: DATE_COL_WIDTH,
                flexShrink: 0,
                fontSize: 12,
                fontWeight: 700,
                color: "var(--text-secondary)",
                paddingRight: 6,
                textAlign: "right",
              }}>
                {formatDateLabel(row.date, t)}
              </div>

              {Array.from({ length: TOP_N }, (_, rankIdx) => {
                const cell = row.entries.find((e) => e.rank === rankIdx + 1);
                if (!cell) {
                  return (
                    <div
                      key={rankIdx}
                      style={{
                        width: CELL_SIZE,
                        height: CELL_SIZE + 10,
                        flexShrink: 0,
                      }}
                    />
                  );
                }
                return (
                  <Cell
                    key={rankIdx}
                    cell={cell}
                    isMe={cell.user_id === userId}
                    onClick={() => openMiniProfile({
                      user_id: cell.user_id,
                      nickname: cell.nickname,
                      avatar_url: cell.avatar_url,
                    })}
                    t={t}
                  />
                );
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Cell({
  cell,
  isMe,
  onClick,
  t,
}: {
  cell: AnnotatedCell;
  isMe: boolean;
  onClick: () => void;
  t: (key: string) => string;
}) {
  return (
    <div
      onClick={onClick}
      title={cell.nickname}
      style={{
        width: CELL_SIZE,
        height: CELL_SIZE + 14,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "flex-start",
        cursor: "pointer",
        borderRadius: 8,
        background: isMe ? "rgba(124, 92, 252, 0.12)" : "transparent",
        border: isMe ? "1px solid rgba(124, 92, 252, 0.25)" : "1px solid transparent",
        padding: 3,
        boxSizing: "border-box",
      }}
    >
      {cell.avatar_url ? (
        <img
          src={cell.avatar_url}
          alt=""
          style={{
            width: AVATAR_SIZE,
            height: AVATAR_SIZE,
            borderRadius: AVATAR_SIZE / 2,
            flexShrink: 0,
          }}
        />
      ) : (
        <div style={{
          width: AVATAR_SIZE,
          height: AVATAR_SIZE,
          borderRadius: AVATAR_SIZE / 2,
          background: "var(--heat-1)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          fontSize: 14,
          fontWeight: 700,
          color: "var(--accent-purple)",
          flexShrink: 0,
        }}>
          {cell.nickname.charAt(0).toUpperCase()}
        </div>
      )}
      <DeltaBadge cell={cell} t={t} />
    </div>
  );
}

function DeltaBadge({ cell, t }: { cell: AnnotatedCell; t: (key: string) => string }) {
  const common: CSSProperties = {
    fontSize: 8,
    fontWeight: 800,
    lineHeight: 1,
    marginTop: 2,
    letterSpacing: "-0.02em",
  };

  if (cell.deltaKind === "new") {
    return (
      <span style={{ ...common, color: "var(--accent-purple)" }}>
        {t("leaderboard.new")}
      </span>
    );
  }
  if (cell.deltaKind === "up") {
    return (
      <span style={{ ...common, color: "var(--accent-mint)" }}>
        ▲{cell.deltaValue}
      </span>
    );
  }
  if (cell.deltaKind === "down") {
    return (
      <span style={{ ...common, color: "var(--accent-pink)" }}>
        ▼{cell.deltaValue}
      </span>
    );
  }
  return (
    <span style={{ ...common, color: "var(--text-secondary)", opacity: 0.5 }}>
      —
    </span>
  );
}

function formatDateLabel(dateStr: string, t: (key: string) => string): string {
  const today = toLocalDateStr(new Date());
  if (dateStr === today) return t("leaderboard.today");
  const yesterday = new Date();
  yesterday.setDate(yesterday.getDate() - 1);
  if (dateStr === toLocalDateStr(yesterday)) return t("leaderboard.yesterday");
  // MM/DD
  const [, mm, dd] = dateStr.split("-");
  return `${mm}/${dd}`;
}
