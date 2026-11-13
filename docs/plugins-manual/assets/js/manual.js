// Sidebar behaviour: filter box, and keep the current page visible. The manual reads fine without it.
(function () {
  var filter = document.querySelector(".filter");
  var sidebar = document.querySelector(".sidebar");
  var current = document.querySelector('.tree a[aria-current="page"]');
  if (current && sidebar) {
    var offset = current.getBoundingClientRect().top - sidebar.getBoundingClientRect().top;
    if (offset > sidebar.clientHeight * 0.6) { sidebar.scrollTop += offset - sidebar.clientHeight * 0.3; }
  }
  if (!filter) { return; }
  var groups = Array.prototype.slice.call(document.querySelectorAll(".tree details"));
  var opened = groups.map(function (group) { return group.open; });
  filter.addEventListener("input", function () {
    var needle = filter.value.trim().toLowerCase();
    groups.forEach(function (group, index) {
      var shown = 0;
      group.querySelectorAll("li").forEach(function (item) {
        var match = !needle || item.textContent.toLowerCase().indexOf(needle) !== -1;
        item.hidden = !match;
        if (match) { shown += 1; }
      });
      group.hidden = needle !== "" && shown === 0;
      group.open = needle ? shown > 0 : opened[index];
    });
  });
})();
