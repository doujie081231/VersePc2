// dino 小游戏初始化：独立文件以满足 CSP script-src 'self'（原内联脚本会被拦截）。
document.onkeydown = function (evt) {
  evt = evt || window.event;
  if (evt.keyCode == 32) {
    var box = document.getElementById('messageBox');
    if (box) box.style.visibility = 'hidden';
  }
};

document.addEventListener('DOMContentLoaded', function () {
  var btn = document.querySelector('.niokbutton');
  if (btn) {
    btn.addEventListener('click', function () {
      if (typeof okbuttonsend === 'function') okbuttonsend();
    });
  }
});
