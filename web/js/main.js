/* ============================================================
   Clyde on Desk — Website Interactions
   Pure vanilla JS, zero dependencies
   ============================================================ */

// ---------- Scroll Animations (IntersectionObserver) ----------
const observer = new IntersectionObserver(
  (entries) => {
    entries.forEach((entry, i) => {
      if (entry.isIntersecting) {
        // Stagger siblings inside grids
        const parent = entry.target.parentElement;
        if (parent && (parent.classList.contains('features-grid') || parent.classList.contains('agents-grid'))) {
          const siblings = Array.from(parent.querySelectorAll('.anim'));
          const idx = siblings.indexOf(entry.target);
          entry.target.style.transitionDelay = `${idx * 80}ms`;
        }
        entry.target.classList.add('visible');
        observer.unobserve(entry.target);
      }
    });
  },
  { threshold: 0.12, rootMargin: '0px 0px -40px 0px' }
);

document.querySelectorAll('.anim').forEach((el) => observer.observe(el));

// ---------- Sticky Header ----------
const header = document.getElementById('site-header');
let lastScroll = 0;
window.addEventListener('scroll', () => {
  header.classList.toggle('scrolled', window.scrollY > 50);
}, { passive: true });

// ---------- Mobile Hamburger ----------
const hamburger = document.getElementById('hamburger');
const nav = document.getElementById('main-nav');
hamburger.addEventListener('click', () => {
  const open = nav.classList.toggle('open');
  hamburger.setAttribute('aria-expanded', open);
});
// Close on nav link click
nav.querySelectorAll('a').forEach((a) => {
  a.addEventListener('click', () => {
    nav.classList.remove('open');
    hamburger.setAttribute('aria-expanded', 'false');
  });
});

// ---------- Smooth Scroll Fallback ----------
document.querySelectorAll('a[href^="#"]').forEach((a) => {
  a.addEventListener('click', (e) => {
    const target = document.querySelector(a.getAttribute('href'));
    if (target) {
      e.preventDefault();
      target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  });
});

// ---------- i18n Toggle ----------
const translations = {
  zh: {
    'nav.features': '功能',
    'nav.howItWorks': '工作原理',
    'nav.agents': 'Agent 支持',
    'nav.download': '下载',
    'hero.pill': '开源 · AGPL-3.0',
    'hero.title': '你的 AI 助手，<br/>活在桌面上',
    'hero.sub': '一只轻量级桌面宠物，实时映射 AI 编程助手的工作状态。提问时思考，跑工具时打字，任务完成时庆祝。',
    'hero.download': '免费下载',
    'hero.bundle': '安装包',
    'hero.startup': '启动速度',
    'hero.memory': '内存占用',
    'features.title': '12 种动画状态，零配置',
    'features.sub': 'Clyde 实时监听 Agent 事件并即时反应。',
    'features.idle.name': '空闲', 'features.idle.desc': '眼球跟随鼠标，身体微倾',
    'features.thinking.name': '思考', 'features.thinking.desc': '提交提示词时进入思考',
    'features.typing.name': '打字', 'features.typing.desc': '工具执行时忙碌打字',
    'features.building.name': '建造', 'features.building.desc': '3+ 个会话同时活跃',
    'features.juggling.name': '杂耍', 'features.juggling.desc': '一个子代理在运行',
    'features.conducting.name': '指挥', 'features.conducting.desc': '两个以上子代理在运行',
    'features.error.name': '报错', 'features.error.desc': '工具执行失败时闪烁报错',
    'features.happy.name': '开心', 'features.happy.desc': '任务完成时开心弹跳',
    'features.notification.name': '通知', 'features.notification.desc': '收到重要通知时跳跃提醒',
    'features.sweeping.name': '扫地', 'features.sweeping.desc': '上下文压缩时打扫',
    'features.carrying.name': '搬运', 'features.carrying.desc': '创建工作区时搬箱子',
    'features.sleeping.name': '睡觉', 'features.sleeping.desc': '60 秒无活动后入睡',
    'hiw.title': '从事件到动画，毫秒级响应',
    'hiw.sub': '一条管线，三个 Agent 来源，十二种反应。',
    'hiw.hook': 'Hook 事件', 'hiw.hookDesc': 'Claude Code 在工具调用、提问或完成时触发 hook',
    'hiw.http': 'HTTP POST', 'hiw.httpDesc': '事件发送到 Clyde 本地服务器 23333 端口',
    'hiw.state': '状态机', 'hiw.stateDesc': '跨所有活跃会话的优先级解析',
    'hiw.anim': '动画', 'hiw.animDesc': 'Clyde 切换到对应的 SVG 动画',
    'agents.title': '三个 Agent，一只宠物',
    'agents.sub': '三者可同时运行，Clyde 独立追踪每个会话。',
    'agents.claude': '零配置 hooks、权限审批气泡、实时模式跟踪',
    'agents.codex': '自动轮询 ~/.codex/sessions/ 下的 JSONL 日志',
    'agents.copilot': '检测到 ~/.copilot 时自动配置',
    'agents.multiTitle': '多会话智能：',
    'agents.multiDesc': '所有会话中最高优先级的状态胜出。1 个子代理 = 杂耍，2+ 个 = 指挥。',
    'perm.title': '审批工具，无需切换窗口',
    'perm.desc': 'Claude Code 请求工具权限时，Clyde 在宠物旁弹出浮动卡片。允许、拒绝或应用建议规则。如果你先在终端回答了，气泡自动消失。',
    'perm.default': '正常审批', 'perm.defaultDesc': '需要批准',
    'perm.accept': '自动编辑', 'perm.acceptDesc': '编辑自动通过',
    'perm.bypass': '跳过审批', 'perm.bypassDesc': '无需审批',
    'perm.plan': '仅规划', 'perm.planDesc': '不执行工具',
    'mini.title': '隐于屏幕边缘',
    'mini.drag': '拖到屏幕边缘进入极简模式',
    'mini.peek': '悬停时探头招手',
    'mini.alerts': '收起状态下仍显示迷你通知',
    'mini.dblclick': '双击戳一下，连点 4 下东张西望',
    'mini.menu': '右键打开会话列表、免打扰、调大小',
    'dl.title': '把 Clyde 放到你的桌面',
    'dl.sub': '免费、开源、约 5 MB。支持 Windows、macOS 和 Linux。',
    'dl.source': '或 <a href="https://github.com/QingJ01/Clyde" target="_blank" rel="noopener">从源码构建</a>',
    'footer.disclaimer': '非 Anthropic 官方产品。Clyde 为社区创作。',
  }
};

const enCache = {};

// Detect initial language: URL param > localStorage > browser language
function detectLang() {
  const params = new URLSearchParams(window.location.search);
  if (params.get('lang') === 'zh' || params.get('lang') === 'en') return params.get('lang');
  const stored = localStorage.getItem('clyde-lang');
  if (stored === 'zh' || stored === 'en') return stored;
  const nav = navigator.language || navigator.userLanguage || '';
  return nav.startsWith('zh') ? 'zh' : 'en';
}

let currentLang = detectLang();

// Cache original English text
document.querySelectorAll('[data-i18n]').forEach((el) => {
  enCache[el.getAttribute('data-i18n')] = el.innerHTML;
});

function setLang(lang) {
  currentLang = lang;
  localStorage.setItem('clyde-lang', lang);
  document.querySelectorAll('[data-i18n]').forEach((el) => {
    const key = el.getAttribute('data-i18n');
    if (lang === 'zh' && translations.zh[key]) {
      el.innerHTML = translations.zh[key];
    } else if (enCache[key]) {
      el.innerHTML = enCache[key];
    }
  });
  document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';
  // Update toggle button text
  document.getElementById('lang-toggle').textContent = lang === 'zh' ? 'EN / 中文' : 'EN / 中文';
}

// Apply detected language on load
if (currentLang === 'zh') setLang('zh');

document.getElementById('lang-toggle').addEventListener('click', () => {
  setLang(currentLang === 'en' ? 'zh' : 'en');
});
