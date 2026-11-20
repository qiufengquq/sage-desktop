import {
  CalendarClock,
  ChartLine,
  ClipboardCheck,
  Dumbbell,
  LayoutDashboard,
  Network,
  Settings,
  Sparkles,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  id: string;
  path: string;
  label: string;
  labelEn: string;
  description: string;
  icon: LucideIcon;
}

export const NAV_ITEMS: NavItem[] = [
  {
    id: "dashboard",
    path: "/",
    label: "首页",
    labelEn: "Dashboard",
    description: "今日任务、进度与快速开始",
    icon: LayoutDashboard,
  },
  {
    id: "assessment",
    path: "/assessment",
    label: "测评",
    labelEn: "Assessment",
    description: "词汇、语法与口语能力检测",
    icon: ClipboardCheck,
  },
  {
    id: "review",
    path: "/review",
    label: "复习",
    labelEn: "Review",
    description: "到期条目的回忆与自评",
    icon: CalendarClock,
  },
  {
    id: "planner",
    path: "/planner",
    label: "学习规划",
    labelEn: "Study Planner",
    description: "基于能力画像的学习计划",
    icon: Sparkles,
  },
  {
    id: "analytics",
    path: "/analytics",
    label: "学习数据",
    labelEn: "Learning Analytics",
    description: "学习曲线、遗忘曲线与复习队列",
    icon: ChartLine,
  },
  {
    id: "knowledge",
    path: "/knowledge",
    label: "知识库",
    labelEn: "Knowledge Base",
    description: "图谱、单词、语法与句子管理",
    icon: Network,
  },
  {
    id: "training",
    path: "/training",
    label: "训练",
    labelEn: "Training",
    description: "词汇、语法与口语训练场",
    icon: Dumbbell,
  },
  {
    id: "settings",
    path: "/settings",
    label: "设置",
    labelEn: "Settings",
    description: "资料、提醒、外观与同步",
    icon: Settings,
  },
];

export function findNavItem(pathname: string): NavItem {
  if (pathname === "/") return NAV_ITEMS[0] as NavItem;
  const match = NAV_ITEMS.find((item) => item.path !== "/" && pathname.startsWith(item.path));
  return match ?? (NAV_ITEMS[0] as NavItem);
}
