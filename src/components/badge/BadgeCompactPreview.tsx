import type { BadgeData } from "../../hooks/useBadgeData";
import { generateFlatBadgeSvg } from "../../lib/badgeSvgTemplate";

interface Props {
  data: BadgeData;
  style: "flat" | "flat-square";
}

export function BadgeCompactPreview({ data, style }: Props) {
  const svg = generateFlatBadgeSvg(
    {
      nickname: data.nickname,
      rank: data.rank,
      totalTokens: data.totalTokens,
      costUsd: data.costUsd,
      provider: data.provider,
      period: data.period,
    },
    style,
  );

  return (
    <div style={{
      display: "flex",
      justifyContent: "center",
      alignItems: "center",
      padding: "20px 24px",
      background: "rgba(255,255,255,0.06)",
      borderRadius: 12,
      border: "1px solid rgba(255,255,255,0.1)",
    }}>
      <div style={{ lineHeight: 0 }} dangerouslySetInnerHTML={{ __html: svg }} />
    </div>
  );
}
